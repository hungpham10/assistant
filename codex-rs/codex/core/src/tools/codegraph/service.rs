//! `CodegraphService` — wraps `codegraph_api::Session` with a process-global
//! session slot (one bound workspace at a time).
//!
//! - `begin(path)`: bind-only (creates `.codegraph/` + config if missing, but
//!   never indexes — indexing is a host action: `codex codegraph index` /
//!   TUI `/index`). Replaces any previously bound session.
//! - `search(action, args)`: query verbs over `ensure_ready()`'s shared index.
//!   Before `begin` → instructional error telling the model to call `begin`.
//!   Empty index → suggests `/index`.
//! - `end()`: release the session. A later `search` fails until `begin` again.
//! - `status()`: bound root + index stats (or "not bound").
//! - `host_index(path, kind)`: host-only index (CLI + `/index`). `kind`
//!   selects what to build: source code, documents, or both. Never exposed
//!   to the agent.

use anyhow::Result;
use camino::Utf8Path;
use camino::Utf8PathBuf;
use serde_json::Value as JsonValue;
use serde_json::json;
use std::env;
use std::path::Path;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::OnceLock;

use codegraph_api::GraphApi;
use codegraph_api::session::DetailLevel;
use codegraph_api::session::Session;
use codegraph_core::Symbol;
use codegraph_core::SymbolKind;
use codegraph_core::SymbolMatch;
use codegraph_docs::DocToken;
use codegraph_extract::ExtractConfig;
use codegraph_extract::SharedDocGraph;
use codegraph_extract::open_doc_graph;

/// Process-global session slot — one conversation at a time. `Arc` so the
/// guard can be dropped before any `.await` (handler futures must be `Send`).
static GLOBAL_SESSION: OnceLock<Mutex<Option<Arc<Session>>>> = OnceLock::new();

fn global_session() -> &'static Mutex<Option<Arc<Session>>> {
    GLOBAL_SESSION.get_or_init(|| Mutex::new(None))
}

/// Process-global auto-init configuration.
static GLOBAL_AUTO_INIT: OnceLock<Mutex<Option<String>>> = OnceLock::new();

fn global_auto_init() -> &'static Mutex<Option<String>> {
    GLOBAL_AUTO_INIT.get_or_init(|| Mutex::new(None))
}

pub fn set_global_auto_init(path: Option<String>) {
    if let Ok(mut guard) = global_auto_init().lock() {
        *guard = path;
    }
}

/// Clone the bound session out (caller awaits without holding the lock).
fn bound_session() -> Option<Arc<Session>> {
    global_session()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone()
}

/// Process-global document-graph slot (lazy-opened per bound root).
static GLOBAL_DOC_GRAPH: OnceLock<Mutex<Option<(Utf8PathBuf, Arc<SharedDocGraph>)>>> =
    OnceLock::new();

fn global_doc_graph() -> &'static Mutex<Option<(Utf8PathBuf, Arc<SharedDocGraph>)>> {
    GLOBAL_DOC_GRAPH.get_or_init(|| Mutex::new(None))
}

/// Default page size for list/search verbs.
const DEFAULT_LIMIT: u32 = 30;
/// Hard cap so a single tool result stays bounded in model context.
const MAX_LIMIT: u32 = 100;

/// What a host index run builds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum IndexKind {
    /// Source code + documents (default).
    #[default]
    All,
    /// Source code only (symbols / call graph).
    Source,
    /// Documents only (`[docgraph] paths` in `.codegraph/config.toml`).
    Docs,
}

impl IndexKind {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "source" | "code" | "src" => Self::Source,
            "docs" | "doc" | "documents" | "document" => Self::Docs,
            _ => Self::All,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Source => "source",
            Self::Docs => "docs",
        }
    }

    pub fn indexes_source(self) -> bool {
        matches!(self, Self::All | Self::Source)
    }

    pub fn indexes_docs(self) -> bool {
        matches!(self, Self::All | Self::Docs)
    }
}

/// Summary returned by a host index run (CLI / `/index`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexSummary {
    pub root: String,
    pub kind: IndexKind,
    pub files: u64,
    pub symbols: u64,
    pub chains: u64,
    pub calls: u64,
    pub skipped: u64,
    /// Whether `[docgraph] paths` were configured (and attempted).
    pub docs_configured: bool,
    /// Documents ingested into the document graph.
    pub docs: usize,
    /// Document-graph nodes after ingest.
    pub doc_nodes: usize,
    /// Document files that failed to ingest.
    pub docs_failed: usize,
}

/// Resolve the configured `auto_init` path against the session workspace root.
/// `None`/empty → `None` (no auto-init, by design); absolute paths are kept
/// as-is; relative paths join `base_cwd` so `auto_init = "."` binds the
/// workspace regardless of the directory the process was launched from.
pub fn resolve_auto_init(base_cwd: &Path, auto_init: Option<String>) -> Option<String> {
    let raw = auto_init?;
    if raw.is_empty() {
        return None;
    }
    let path = Path::new(&raw);
    if path.is_absolute() {
        return Some(raw);
    }
    let joined = base_cwd.join(path);
    // Drop `.` components lexically so `auto_init = "."` resolves to the
    // workspace root itself rather than `/root/.`.
    let cleaned: std::path::PathBuf = joined
        .components()
        .filter(|component| *component != std::path::Component::CurDir)
        .collect();
    Some(cleaned.to_string_lossy().to_string())
}

pub struct CodegraphService;

impl CodegraphService {
    /// Attempt to auto-initialize from config if not already bound.
    /// If `auto_init` is Some, resolve the path (relative to cwd if not absolute)
    /// and call begin() if not currently bound.
    pub async fn auto_init_from_config(auto_init: Option<&str>) -> Option<String> {
        auto_init?;
        // If already bound, don't re-init
        if bound_session().is_some() {
            return None;
        }
        let path_str = auto_init.unwrap_or("");
        if path_str.is_empty() {
            return None;
        }
        let path = Path::new(path_str);
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            match env::current_dir() {
                Ok(cwd) => cwd.join(path),
                Err(_) => return Some("auto_init failed: cannot get current dir".to_string()),
            }
        };
        let resolved_str = resolved.to_string_lossy().to_string();
        let result = Self::begin(&resolved_str).await;
        Some(result)
    }

    /// `{"verb":"begin","path":...}` — bind-only, never indexes.
    pub async fn begin(path: &str) -> String {
        let session = Arc::new(Session::new());
        let outcome = match session
            .init(
                Utf8PathBuf::from(path),
                /*do_index*/ false,
                DetailLevel::Medium,
                /*format*/ None,
            )
            .await
        {
            Ok(outcome) => outcome,
            Err(err) => return format!("begin failed: {err:#}"),
        };
        let root = outcome.root.clone();
        *global_session()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Arc::clone(&session));

        let stats = index_stats().await;
        let doc_stats = doc_stats().await;
        let empty = stats.is_none_or(|s| s.symbols == 0) && doc_stats.is_none_or(|s| s.docs == 0);
        let hint = if empty {
            " Index is empty — run `/index` (or `codex codegraph index`) in the host, then call begin again."
        } else {
            ""
        };
        format!(
            "begin ok: bound to {} (dir {}).{}",
            root.as_str(),
            outcome.dir.as_str(),
            hint
        )
    }

    /// `{"verb":"end"}` — release the session.
    pub async fn end() -> String {
        *global_doc_graph()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        let session = global_session()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        match session {
            Some(session) => {
                let root = session.deinit().await.ok().flatten();
                match root {
                    Some(r) => format!("end ok: released {}", r.as_str()),
                    None => "end ok: no session was bound".to_string(),
                }
            }
            None => "end ok: no session was bound".to_string(),
        }
    }

    /// `{"verb":"status"}` — bound root + stats, or "not bound".
    pub async fn status() -> String {
        // Try lazy auto-init if configured
        if bound_session().is_none() {
            let auto_init = global_auto_init().lock().ok().and_then(|g| g.clone());
            if auto_init.is_some() {
                let _ = Self::auto_init_from_config(auto_init.as_deref()).await;
            }
        }
        let Some(session) = bound_session() else {
            return "status: not bound — call begin(path=...) first (or set [codegraph] auto_init in config.toml)"
                .to_string();
        };
        let root = session.root().await;
        let initialized = session.is_initialized().await;
        let stats_line = if initialized {
            let source = match index_stats().await {
                Some(s) => format!(
                    " files={} symbols={} chains={} edges={}",
                    s.files, s.symbols, s.chains, s.edges
                ),
                None => " source_stats=unavailable".to_string(),
            };
            let docs = match doc_stats().await {
                Some(d) => format!(" docs={} doc_nodes={}", d.docs, d.nodes),
                None => " docs=unavailable".to_string(),
            };
            format!("{source}{docs}")
        } else {
            " initialized=false".to_string()
        };
        format!(
            "status: bound={} initialized={}{}",
            root.map(|r| r.as_str().to_string())
                .unwrap_or_else(|| "<none>".to_string()),
            initialized,
            stats_line
        )
    }

    /// `{"verb":"search","action":...}` — dispatch one query verb.
    pub async fn search(action: &str, args: &JsonValue) -> String {
        if bound_session().is_none() {
            let auto_init = global_auto_init().lock().ok().and_then(|g| g.clone());
            if auto_init.is_some() {
                let _ = Self::auto_init_from_config(auto_init.as_deref()).await;
            }
        }
        if bound_session().is_none() {
            return format!(
                "error: no codegraph session bound — call begin(path=...) first, then search. (requested action: {action})"
            );
        }
        if matches!(action, "doc_search" | "doc_list" | "doc_stats") {
            return match dispatch_doc_search(action, args).await {
                Ok(out) => out,
                Err(err) => format!("search({action}) failed: {err:#}"),
            };
        }
        let api = match graph_api().await {
            Ok(api) => api,
            Err(err) => return format!("search failed: {err:#}"),
        };
        match dispatch_search(&api, action, args).await {
            Ok(out) => out,
            Err(err) => format!("search({action}) failed: {err:#}"),
        }
    }

    /// Host-only index of `kind` (source / docs / both). Used by the CLI
    /// (`codex codegraph index`) and the TUI `/index` command — never by the
    /// agent tool. `path` is created/bound (`.codegraph/` + config) if missing.
    pub async fn host_index(path: &str, kind: &str) -> Result<IndexSummary> {
        let kind = IndexKind::parse(kind);
        let session = Session::new();
        let outcome = session
            .init(
                Utf8PathBuf::from(path),
                /*do_index*/ kind.indexes_source(),
                DetailLevel::Medium,
                /*format*/ None,
            )
            .await?;
        let s = outcome.indexed.unwrap_or(codegraph_extract::ExtractStats {
            files: 0,
            symbols: 0,
            chains: 0,
            calls: 0,
            skipped: 0,
        });
        let docs = if kind.indexes_docs() {
            ingest_docs(&outcome.root).await?
        } else {
            DocsSummary::default()
        };
        Ok(IndexSummary {
            root: outcome.root.as_str().to_string(),
            kind,
            files: s.files,
            symbols: s.symbols,
            chains: s.chains,
            calls: s.calls,
            skipped: s.skipped,
            docs_configured: docs.configured,
            docs: docs.docs,
            doc_nodes: docs.doc_nodes,
            docs_failed: docs.failed,
        })
    }
}

/// Result of ingesting `[docgraph] paths` into the document graph.
#[derive(Debug, Default)]
struct DocsSummary {
    configured: bool,
    docs: usize,
    doc_nodes: usize,
    failed: usize,
}

/// Ingest every file matched by `[docgraph] paths` (idempotent — doc paths
/// are replaced in place). No `paths` configured → no-op.
async fn ingest_docs(root: &Utf8Path) -> Result<DocsSummary> {
    let Some((_, files)) = ExtractConfig::load(root).doc_config(root) else {
        return Ok(DocsSummary::default());
    };
    let mut graph = open_doc_graph(root).await?;
    let mut summary = DocsSummary {
        configured: true,
        ..Default::default()
    };
    for (path, format) in &files {
        match graph.ingest_file(path.as_str(), format.as_deref()).await {
            Ok(_) => {}
            Err(_) => summary.failed += 1,
        }
    }
    let stats = graph.stats().await.unwrap_or_default();
    summary.docs = stats.docs;
    summary.doc_nodes = stats.nodes;
    Ok(summary)
}

/// Build a `GraphApi` over the bound session's shared index.
async fn graph_api() -> Result<GraphApi> {
    let session = bound_session().ok_or_else(|| {
        anyhow::anyhow!("no codegraph session bound — call begin(path=...) first")
    })?;
    let shared = session.ensure_ready().await?;
    Ok(GraphApi::new_with_index(shared))
}

/// Stats of the bound index, if it can be loaded.
async fn index_stats() -> Option<codegraph_core::SemgraphStats> {
    let api = graph_api().await.ok()?;
    Some(api.stats_cached().await)
}

/// Document graph for the bound root (lazy-opened once, cached per root).
async fn doc_graph() -> Result<Arc<SharedDocGraph>> {
    let session = bound_session().ok_or_else(|| {
        anyhow::anyhow!("no codegraph session bound — call begin(path=...) first")
    })?;
    let root = session
        .root()
        .await
        .ok_or_else(|| anyhow::anyhow!("no codegraph session bound"))?;
    let mut slot = global_doc_graph()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((cached_root, graph)) = slot.as_ref()
        && *cached_root == root
    {
        return Ok(Arc::clone(graph));
    }
    let graph = Arc::new(SharedDocGraph::lazy(root.clone()));
    *slot = Some((root, Arc::clone(&graph)));
    Ok(graph)
}

/// Document stats of the bound root, if the graph can be opened.
#[allow(clippy::await_holding_invalid_type)]
async fn doc_stats() -> Option<codegraph_docs::DocStats> {
    let shared = doc_graph().await.ok()?;
    let graph = shared.graph().await;
    let stats = graph.read().await.stats().await;
    stats.ok()
}

fn arg_str<'a>(args: &'a JsonValue, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

fn arg_u64(args: &JsonValue, key: &str) -> Option<u64> {
    args.get(key).and_then(serde_json::Value::as_u64)
}

fn arg_u32(args: &JsonValue, key: &str) -> Option<u32> {
    arg_u64(args, key).map(|v| v as u32)
}

fn limit_arg(args: &JsonValue) -> u32 {
    arg_u32(args, "limit")
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT)
}

fn kind_arg(args: &JsonValue) -> Result<Option<SymbolKind>> {
    match arg_str(args, "kind") {
        Some(s) => SymbolKind::parse(s)
            .map(Some)
            .ok_or_else(|| anyhow::anyhow!("unknown symbol kind: {s}")),
        None => Ok(None),
    }
}

fn mode_arg(args: &JsonValue) -> Result<SymbolMatch> {
    match arg_str(args, "mode") {
        Some(s) => SymbolMatch::parse(s).ok_or_else(|| anyhow::anyhow!("unknown match mode: {s}")),
        None => Ok(SymbolMatch::Contains),
    }
}

/// Compact symbol view (`{id,name,kind,file,line,signature}`) — bounded and
/// token-friendly for model context.
fn symbol_json(s: &Symbol) -> JsonValue {
    json!({
        "id": s.id,
        "name": s.name,
        "kind": s.kind.as_str(),
        "file": s.file,
        "line": s.line,
        "signature": s.signature,
    })
}

fn symbols_json(symbols: &[Symbol], total: usize) -> String {
    serde_json::to_string(&json!({
        "symbols": symbols.iter().map(symbol_json).collect::<Vec<_>>(),
        "total": total,
        "returned": symbols.len(),
    }))
    .unwrap_or_else(|err| format!("{{\"error\":\"failed to serialize symbols: {err}\"}}"))
}

/// Document-graph verbs: `doc_search` (full-path trie search with key /
/// fuzzy fallback), `doc_list`, `doc_stats`.
#[allow(clippy::await_holding_invalid_type)]
async fn dispatch_doc_search(action: &str, args: &JsonValue) -> Result<String> {
    let shared = doc_graph().await?;
    let graph = shared.graph().await;
    match action {
        "doc_list" => {
            let docs = graph.read().await.list_docs();
            let total = docs.len();
            let docs: Vec<JsonValue> = docs
                .into_iter()
                .take(limit_arg(args) as usize)
                .filter_map(|d| serde_json::to_value(&d).ok())
                .collect();
            Ok(serde_json::to_string(&json!({
                "docs": docs, "total": total, "returned": docs.len(),
            }))?)
        }
        "doc_stats" => {
            let stats = {
                let guard = graph.read().await;
                guard.stats().await?
            };
            Ok(serde_json::to_string(&serde_json::to_value(&stats)?)?)
        }
        "doc_search" => {
            let pattern = arg_str(args, "query")
                .or_else(|| arg_str(args, "pattern"))
                .ok_or_else(|| anyhow::anyhow!("missing `query`"))?;
            let depth = arg_u32(args, "depth").unwrap_or(1) as usize;
            let limit = limit_arg(args) as usize;
            #[allow(clippy::await_holding_invalid_type)]
            let graph = graph.read().await;
            let mut tokens = vec![DocToken::root()];
            let mut unknown_seg = false;
            for seg in pattern.split('.') {
                match graph.intern_id(seg) {
                    Some(id) => tokens.push(DocToken::field(id)),
                    None => {
                        unknown_seg = true;
                        break;
                    }
                }
            }
            let ids = if !unknown_seg {
                let mut ids = graph
                    .search_path(&tokens, Some(tokens.len() - 1 + depth))
                    .await
                    .unwrap_or_default();
                ids.extend(graph.search_path_scan(&tokens, 100));
                ids.sort_unstable();
                ids.dedup();
                ids
            } else {
                Vec::new()
            };
            if ids.is_empty() {
                let last = pattern.rsplit('.').next().unwrap_or(pattern);
                let exact = graph.search_key_substring(last, limit);
                if !exact.is_empty() {
                    let results: Vec<JsonValue> = exact
                        .iter()
                        .map(|n| {
                            json!({
                                "id": n.id, "doc": n.doc, "key": n.key,
                                "kind": format!("{:?}", n.kind), "value": n.value,
                            })
                        })
                        .collect();
                    return Ok(serde_json::to_string(&json!({
                        "results": results, "total": results.len(),
                    }))?);
                }
                let hits = graph.search_key_fuzzy(last, limit);
                if hits.is_empty() {
                    return Ok(format!(
                        "no document nodes matched — key `{last}` not seen in any ingested document"
                    ));
                }
                let results: Vec<JsonValue> = hits
                    .iter()
                    .map(|h| {
                        json!({
                            "id": h.node.id, "doc": h.node.doc,
                            "key": h.matched_key, "matched_key": h.matched_key,
                            "score": (h.score * 1000.0).round() / 1000.0,
                            "kind": format!("{:?}", h.node.kind), "value": h.node.value,
                        })
                    })
                    .collect();
                return Ok(serde_json::to_string(&json!({
                    "results": results, "total": results.len(),
                }))?);
            }
            let mut results = Vec::new();
            for id in ids.iter().take(limit) {
                if let Some(payload) = graph.hydrate_depth(*id, Some(1)).await
                    && let Ok(value) = serde_json::to_value(&payload)
                {
                    results.push(value);
                }
            }
            Ok(serde_json::to_string(&json!({
                "results": results, "total": results.len(),
            }))?)
        }
        other => anyhow::bail!("unknown document action: {other}"),
    }
}

async fn dispatch_search(api: &GraphApi, action: &str, args: &JsonValue) -> Result<String> {
    match action {
        "search_symbol" => {
            let query = arg_str(args, "query").ok_or_else(|| anyhow::anyhow!("missing `query`"))?;
            let kind = kind_arg(args)?;
            let mode = mode_arg(args)?;
            let out = api
                .search_symbol_paged_resumable(
                    query,
                    kind,
                    mode,
                    codegraph_api::Pagination {
                        limit: limit_arg(args),
                        offset: arg_u32(args, "offset").unwrap_or(0),
                    },
                    /*resume*/ None,
                    /*timeout_ms*/ 0,
                )
                .await?;
            Ok(symbols_json(&out.page, out.total))
        }
        "symbol" => {
            let id = arg_u64(args, "id").ok_or_else(|| anyhow::anyhow!("missing `id`"))?;
            match api.symbol_by_id(id).await {
                Some(s) => Ok(serde_json::to_string(&symbol_json(&s))?),
                None => Ok(format!("no symbol with id {id}")),
            }
        }
        "resolve" => {
            let name = arg_str(args, "name").unwrap_or("");
            let id = arg_u64(args, "id").unwrap_or(0);
            let r = api.resolve(name, id).await?;
            Ok(serde_json::to_string(&serde_json::to_value(&r)?)?)
        }
        "callers" => {
            let id = arg_u64(args, "id").ok_or_else(|| anyhow::anyhow!("missing `id`"))?;
            let depth = arg_u32(args, "depth").unwrap_or(1).max(1);
            let callers = api.callers(id, depth).await?;
            let total = callers.len();
            Ok(symbols_json(&callers, total))
        }
        "callees" => {
            let id = arg_u64(args, "id").ok_or_else(|| anyhow::anyhow!("missing `id`"))?;
            let callees = api.callees(id).await?;
            let total = callees.len();
            Ok(symbols_json(&callees, total))
        }
        "impact" => {
            let id = arg_u64(args, "id").ok_or_else(|| anyhow::anyhow!("missing `id`"))?;
            let max_depth = arg_u32(args, "max_depth")
                .or_else(|| arg_u32(args, "depth"))
                .unwrap_or(3)
                .max(1);
            let impacted = api.impact(id, max_depth).await?;
            let total = impacted.len();
            Ok(symbols_json(&impacted, total))
        }
        "flow" => {
            let id = arg_u64(args, "id").ok_or_else(|| anyhow::anyhow!("missing `id`"))?;
            let flow = api.flow(id).await?;
            Ok(serde_json::to_string(&serde_json::to_value(&flow)?)?)
        }
        "trace_full_flow" => {
            let id = arg_u64(args, "id").ok_or_else(|| anyhow::anyhow!("missing `id`"))?;
            let max_depth = arg_u32(args, "max_depth")
                .or_else(|| arg_u32(args, "depth"))
                .unwrap_or(10) as usize;
            let max_paths = arg_u32(args, "max_paths").unwrap_or(10) as usize;
            let max_loop_iter = arg_u32(args, "max_loop_iter").unwrap_or(100) as usize;
            let traces = api
                .trace_full_flow(id, max_depth, max_paths, max_loop_iter)
                .await?;
            Ok(serde_json::to_string(&serde_json::to_value(&traces)?)?)
        }
        "context" => {
            let query = arg_str(args, "query").ok_or_else(|| anyhow::anyhow!("missing `query`"))?;
            let mut request = codegraph_context::ContextRequest::default();
            request.query = query.to_string();
            request.depth = arg_u32(args, "depth").unwrap_or(request.depth).max(1);
            request.limit = arg_u32(args, "limit").unwrap_or(request.limit).max(1);
            request.include_source = args
                .get("include_source")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(request.include_source);
            let markdown = api.context_markdown(&request, /*source*/ None).await?;
            Ok(markdown)
        }
        "search_flow" => {
            let pattern =
                arg_str(args, "pattern").ok_or_else(|| anyhow::anyhow!("missing `pattern`"))?;
            let results = api.search_flow_pattern(pattern).await?;
            Ok(serde_json::to_string(&serde_json::to_value(&results)?)?)
        }
        "references" => {
            let query = arg_str(args, "query").ok_or_else(|| anyhow::anyhow!("missing `query`"))?;
            let refs = api.references(query, limit_arg(args)).await?;
            Ok(serde_json::to_string(&serde_json::to_value(&refs)?)?)
        }
        "list_by_kind" => {
            let kind = kind_arg(args)?.ok_or_else(|| anyhow::anyhow!("missing `kind`"))?;
            let (page, total) = api
                .list_by_kind(kind, limit_arg(args), arg_u32(args, "offset").unwrap_or(0))
                .await;
            Ok(symbols_json(&page, total))
        }
        "files" => {
            let prefix = arg_str(args, "prefix").unwrap_or("");
            let files = api.files(prefix).await;
            let total = files.len();
            let files: Vec<JsonValue> = files
                .into_iter()
                .take(limit_arg(args) as usize)
                .filter_map(|f| serde_json::to_value(&f).ok())
                .collect();
            Ok(serde_json::to_string(&json!({
                "files": files, "total": total, "returned": files.len(),
            }))?)
        }
        "stats" => {
            let stats = api.stats_cached().await;
            Ok(serde_json::to_string(&serde_json::to_value(&stats)?)?)
        }
        other => anyhow::bail!(
            "unknown search action: {other} (available: search_symbol, symbol, resolve, callers, callees, impact, flow, search_flow, references, list_by_kind, files, stats, doc_search, doc_list, doc_stats)"
        ),
    }
}

#[cfg(test)]
#[path = "service_tests.rs"]
mod tests;
