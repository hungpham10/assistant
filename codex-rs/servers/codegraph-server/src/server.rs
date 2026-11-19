//! In-process MCP server surface over the codegraph data layer.
//!
//! Tools are flat MCP functions with rich descriptions so tool-selection
//! systems (Jev) and the model see full per-tool signal — the reason this
//! surface replaced the builtin namespace handlers. The transport is served
//! on an in-process duplex stream (see [`serve_on`]); indexing stays a host
//! action (`codex codegraph index` / TUI `/index`).

use std::borrow::Cow;
use std::sync::Arc;

use rmcp::ErrorData as McpError;
use rmcp::ServerHandler;
use rmcp::model::CallToolRequestParams;
use rmcp::model::CallToolResult;
use rmcp::model::Content;
use rmcp::model::JsonObject;
use rmcp::model::ListToolsResult;
use rmcp::model::PaginatedRequestParams;
use rmcp::model::ServerCapabilities;
use rmcp::model::ServerInfo;
use rmcp::model::Tool;
use serde_json::json;

use crate::service::CodegraphService;

/// Tool name → rich description. Single source of truth for `list_tools`
/// and tests.
const TOOL_DESCRIPTIONS: &[(&str, &str)] = &[
    (
        "search_symbol",
        "Search code symbols by name — exact, prefix, suffix or semantic similarity (works when \
         you don't remember the exact spelling). Returns ranked matches with kind, file, line and \
         signature. Prefer over grep/rg for locating definitions, usages or related symbols: \
         results are ranked, typed and paginated instead of raw text matches.",
    ),
    (
        "resolve",
        "Resolve a symbol by name or id to its canonical codegraph identity (id, kind, file, \
         line). Use first when other codegraph tools need a symbol id and you only have a name.",
    ),
    (
        "callers",
        "Find every function that calls a symbol — exact call sites from the call graph, not text \
         matches. Use for 'who calls X', impact review before a change, or tracing a feature's \
         entry points. Transitive depth supported.",
    ),
    (
        "callees",
        "Find the functions a symbol calls (downstream call targets) — exact call-graph edges. \
         Use to understand what a function does step by step or to walk into an implementation.",
    ),
    (
        "context",
        "Composed context for a symbol or topic: search + callers + callees in one payload, \
         optionally with source. Best first tool for 'how does X work' / 'what is Y' questions — \
         returns the conclusion instead of file dumps.",
    ),
    (
        "files",
        "List indexed files in the codebase, optionally filtered by path prefix. Use to discover \
         where a module lives before drilling into its symbols.",
    ),
    (
        "flow",
        "Control flow and call chain for a symbol: ordered call/return/branch steps with callee \
         names, line numbers, guard conditions and effects. Use to understand how a function \
         executes without reading the whole file.",
    ),
    (
        "trace_full_flow",
        "Trace full execution paths from callers through to a target symbol, including \
         call/return/throw and branch information. Use for 'how does execution reach X' and \
         failure-path analysis.",
    ),
    (
        "impact",
        "Transitive dependency radius of a symbol — what breaks if you change it. Use before \
         refactoring or renaming.",
    ),
    (
        "symbol",
        "Full details for one symbol (kind, file, line, signature). Use after search_symbol to \
         inspect a specific hit, or by id from other codegraph results.",
    ),
    (
        "search_flow",
        "Find functions whose call chain contains a pattern (comma-separated marker and symbol \
         tokens, e.g. 'LOOP,THROW'). Use for structural questions like 'which functions throw \
         inside a loop'.",
    ),
    (
        "references",
        "Find functions that call a library/API whose name contains the query (includes \
         unresolved external calls). Use for 'where do we use this dependency' questions.",
    ),
    (
        "list_by_kind",
        "List symbols of one kind (function, method, class, enum, ...) paginated. Use to browse \
         a category across the indexed codebase.",
    ),
    (
        "stats",
        "Codegraph index statistics (files, symbols, chains, edges). Use to check index coverage \
         before trusting empty results.",
    ),
    (
        "mermaid",
        "Render a Mermaid call-graph diagram around a symbol: callers (upstream), callees \
         (downstream), impact (transitive upstream) or control flow. Use to present or reason \
         about structure visually; returns diagram text, not file dumps.",
    ),
    (
        "begin",
        "Bind the codegraph session to a workspace path (creates .codegraph/ if missing, never \
         indexes). Indexing is host-only: run `codex codegraph index` or /index, then begin \
         again.",
    ),
    (
        "end",
        "Release the codegraph session. A later search fails until begin again.",
    ),
    (
        "status",
        "Codegraph session status: bound root and index stats, or 'not bound'. Use to diagnose \
         the begin/end lifecycle state.",
    ),
    (
        "doc_search",
        "Search ingested documents (YAML/JSON/TOML/HCL manifests) by dotted key path with \
         substring and fuzzy-key fallback. Use for 'where is this config key defined' questions \
         across many manifests.",
    ),
    (
        "doc_list",
        "List ingested documents with id, path, format and node counts. Use to see what the \
         document graph covers.",
    ),
    (
        "doc_stats",
        "Document graph statistics (documents and nodes). Use to check document coverage.",
    ),
    (
        "doc_hydrate",
        "Hydrate a document node into a small payload (path, kind, value, key, children) up to \
         max_depth. Use after a search hit to inspect the surrounding structure without reading \
         the whole file.",
    ),
    (
        "doc_struct",
        "Search document nodes by structural kind chain (e.g. 'MAP,FIELD,NUMBER'), ranked by IDF \
         so rare/characteristic structures surface first. Use when you know the shape but not \
         the key name.",
    ),
    (
        "doc_mine_patterns",
        "Mine structural patterns across all ingested documents: kind chains ending at scalar \
         leaves, sorted by rarity. Use to discover recurring config shapes or outliers.",
    ),
    (
        "doc_list_patterns",
        "List the mined structural pattern registry (pattern id, kind tokens, node count, doc \
         frequency). Run mine_patterns first.",
    ),
];

fn tool_schema(schema: serde_json::Value) -> Arc<JsonObject> {
    match schema {
        serde_json::Value::Object(map) => Arc::new(map),
        other => panic!("tool schema should be a JSON object, got {other}"),
    }
}

fn tool(name: &str, schema: serde_json::Value) -> Tool {
    let description = TOOL_DESCRIPTIONS
        .iter()
        .find(|(tool, _)| *tool == name)
        .map(|(_, description)| *description)
        .unwrap_or_else(|| panic!("missing description for codegraph tool {name}"));
    Tool::new(
        Cow::Owned(name.to_string()),
        Cow::Owned(description.to_string()),
        tool_schema(schema),
    )
}

/// Static tool definitions served by `list_tools`.
fn codegraph_tools() -> Vec<Tool> {
    let str_prop = |description: &str| json!({"type": "string", "description": description});
    let int_prop = |description: &str| json!({"type": "integer", "description": description});
    let object = |props: serde_json::Value, required: &[&str]| {
        let mut value = json!({
            "type": "object",
            "properties": props,
            "additionalProperties": false,
        });
        if !required.is_empty() {
            value["required"] = json!(required);
        }
        value
    };

    vec![
        tool(
            "search_symbol",
            object(
                json!({
                    "query": str_prop("search query"),
                    "mode": str_prop("match mode: contains|prefix|suffix|exact|semantic|hybrid"),
                    "kind": str_prop("symbol kind filter (function, method, class, enum, ...)"),
                    "limit": int_prop("max results"),
                    "offset": int_prop("pagination offset"),
                }),
                &["query"],
            ),
        ),
        tool(
            "resolve",
            object(
                json!({
                    "name": str_prop("symbol name (if id not provided)"),
                    "id": int_prop("symbol id (if name not provided)"),
                }),
                &[],
            ),
        ),
        tool(
            "callers",
            object(
                json!({
                    "id": int_prop("symbol id"),
                    "depth": int_prop("call depth (default 1)"),
                }),
                &["id"],
            ),
        ),
        tool(
            "callees",
            object(json!({"id": int_prop("symbol id")}), &["id"]),
        ),
        tool(
            "context",
            object(
                json!({
                    "query": str_prop("symbol name or topic"),
                    "depth": int_prop("BFS hops (default 1)"),
                    "limit": int_prop("max results (default 5)"),
                    "include_source": json!({"type": "boolean", "description": "include source snippets"}),
                }),
                &["query"],
            ),
        ),
        tool(
            "files",
            object(
                json!({
                    "prefix": str_prop("path prefix filter"),
                    "limit": int_prop("max results"),
                }),
                &[],
            ),
        ),
        tool(
            "flow",
            object(json!({"id": int_prop("symbol id")}), &["id"]),
        ),
        tool(
            "trace_full_flow",
            object(
                json!({
                    "id": int_prop("symbol id"),
                    "max_depth": int_prop("max caller hops (default 10)"),
                    "max_paths": int_prop("max paths (default 10)"),
                    "max_loop_iter": int_prop("max loop iterations (default 100)"),
                }),
                &["id"],
            ),
        ),
        tool(
            "impact",
            object(
                json!({
                    "id": int_prop("symbol id"),
                    "max_depth": int_prop("max dependency depth (default 3)"),
                }),
                &["id"],
            ),
        ),
        tool(
            "symbol",
            object(json!({"id": int_prop("symbol id")}), &["id"]),
        ),
        tool(
            "search_flow",
            object(
                json!({"pattern": str_prop("comma-separated tokens, e.g. LOOP,THROW")}),
                &["pattern"],
            ),
        ),
        tool(
            "references",
            object(
                json!({
                    "query": str_prop("library call name substring"),
                    "limit": int_prop("max results"),
                }),
                &["query"],
            ),
        ),
        tool(
            "list_by_kind",
            object(
                json!({
                    "kind": str_prop("symbol kind (function, method, class, enum, ...)"),
                    "limit": int_prop("max results"),
                    "offset": int_prop("pagination offset"),
                }),
                &["kind"],
            ),
        ),
        tool("stats", object(json!({}), &[])),
        tool(
            "mermaid",
            object(
                json!({
                    "id": int_prop("symbol id"),
                    "kind": str_prop("diagram kind: flow|callers|callees|impact"),
                    "depth": int_prop("graph depth (1-5, default 2)"),
                }),
                &["id"],
            ),
        ),
        tool(
            "begin",
            object(json!({"path": str_prop("workspace path")}), &["path"]),
        ),
        tool("end", object(json!({}), &[])),
        tool("status", object(json!({}), &[])),
        tool(
            "doc_search",
            object(
                json!({
                    "query": str_prop("dotted key path, e.g. spec.replicas"),
                    "limit": int_prop("max results"),
                }),
                &["query"],
            ),
        ),
        tool("doc_list", object(json!({}), &[])),
        tool("doc_stats", object(json!({}), &[])),
        tool(
            "doc_hydrate",
            object(
                json!({
                    "node_id": int_prop("document node id"),
                    "max_depth": int_prop("max child levels (default unlimited)"),
                }),
                &["node_id"],
            ),
        ),
        tool(
            "doc_struct",
            object(
                json!({
                    "pattern": str_prop(
                        "comma-separated kind chain: MAP,ARRAY,FIELD,INDEX,STRING,NUMBER,BOOL,NULL,ROOT",
                    ),
                    "depth": int_prop("search depth"),
                    "limit": int_prop("max results"),
                }),
                &["pattern"],
            ),
        ),
        tool(
            "doc_mine_patterns",
            object(
                json!({
                    "top_k": int_prop("max patterns (default 20)"),
                    "min_count": int_prop("min node occurrences (default 3)"),
                    "max_depth": int_prop("max chain window (1-8, default 4)"),
                }),
                &[],
            ),
        ),
        tool("doc_list_patterns", object(json!({}), &[])),
    ]
}

/// MCP server handler over [`CodegraphService`]. Stateless: the service
/// keeps the process-global session.
#[derive(Default)]
pub struct CodegraphServerHandler;

impl ServerHandler for CodegraphServerHandler {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        let tools = Arc::new(codegraph_tools());
        async move {
            Ok(ListToolsResult {
                tools: (*tools).clone(),
                next_cursor: None,
                meta: None,
            })
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: rmcp::service::RequestContext<rmcp::service::RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let name = request.name.as_ref();
        let mut args = match request.arguments {
            Some(arguments) => serde_json::Value::Object(arguments.into_iter().collect()),
            None => json!({}),
        };
        if !args.is_object() {
            args = json!({});
        }
        let output = self.dispatch(name, &args).await;
        Ok(CallToolResult::success(vec![Content::text(output)]))
    }
}

impl CodegraphServerHandler {
    async fn dispatch(&self, name: &str, args: &serde_json::Value) -> String {
        match name {
            "begin" => {
                let path = args.get("path").and_then(|v| v.as_str()).unwrap_or("");
                CodegraphService::begin(path).await
            }
            "end" => CodegraphService::end().await,
            "status" => CodegraphService::status().await,
            other => CodegraphService::search(other, args).await,
        }
    }
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
