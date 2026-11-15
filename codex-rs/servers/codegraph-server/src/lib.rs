//! `codex-codegraph-server` — in-process MCP server over the vendored
//! codegraph data layer (`codegraph_api::Session` + friends).
//!
//! codex registers this as a built-in compatibility MCP server (like
//! `codex_apps`) when `[codegraph] enabled`: the connection runs on an
//! in-process duplex stream instead of a spawned subprocess, but the tools
//! are plain flat MCP functions — full descriptions, standard exposure,
//! tool-search and Jev all see them like any MCP server. Indexing remains a
//! host action (`codex codegraph index` / TUI `/index`).

pub mod server;
pub mod service;

pub use server::CodegraphServerHandler;
pub use service::CodegraphService;
pub use service::IndexKind;
pub use service::IndexSummary;
pub use service::resolve_auto_init;
pub use service::set_global_auto_init;

use rmcp::ServiceExt;
use tokio::io::DuplexStream;

/// Env var the host sets on the embedded server config to carry
/// `[codegraph] auto_init`.
pub const CODEGRAPH_MCP_AUTO_INIT_ENV_VAR: &str = "CODEX_CODEGRAPH_AUTO_INIT";

/// Serve the codegraph MCP server on one end of an in-process duplex stream.
/// The other end is handed to the rmcp client (see
/// `codex_rmcp_client::RmcpClient::new_in_process_client`).
///
/// `auto_init` mirrors `[codegraph] auto_init`: bound on first use if no
/// session is open yet.
pub async fn serve_on(stream: DuplexStream, auto_init: Option<String>) -> anyhow::Result<()> {
    set_global_auto_init(auto_init);
    let running = CodegraphServerHandler.serve(stream).await?;
    running.waiting().await?;
    Ok(())
}
