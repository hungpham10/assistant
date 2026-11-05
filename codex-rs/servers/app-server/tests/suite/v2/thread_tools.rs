use anyhow::Result;
use app_test_support::TestAppServer;
use app_test_support::to_response;
use codex_app_server_protocol::RequestId;
use codex_app_server_protocol::ThreadSetToolEnabledParams;
use codex_app_server_protocol::ThreadSetToolEnabledResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::ThreadStartResponse;
use core_test_support::skip_if_no_network;
use std::time::Duration;
use tokio::time::timeout;

const DEFAULT_READ_TIMEOUT: Duration = Duration::from_secs(15);

async fn start_thread(mcp: &mut TestAppServer) -> Result<String> {
    let thread_req = mcp
        .send_thread_start_request(ThreadStartParams::default())
        .await?;
    let thread_resp = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(thread_req)),
    )
    .await??;
    let ThreadStartResponse { thread, .. } = to_response::<ThreadStartResponse>(thread_resp)?;
    Ok(thread.id)
}

/// The TUI awaits this request synchronously in its event loop; if the server
/// never responds the whole UI freezes. The round-trip must complete quickly.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_set_tool_enabled_round_trips() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let mut mcp = TestAppServer::builder().build().await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;
    let thread_id = start_thread(&mut mcp).await?;

    let request_id = mcp
        .send_thread_set_tool_enabled_request(ThreadSetToolEnabledParams {
            thread_id,
            name: "apply_patch".to_string(),
            enabled: false,
        })
        .await?;
    let response = timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_response_message(RequestId::Integer(request_id)),
    )
    .await??;
    let _: ThreadSetToolEnabledResponse = to_response::<ThreadSetToolEnabledResponse>(response)?;

    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn thread_set_tool_enabled_rejects_unknown_thread() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let mut mcp = TestAppServer::builder().build().await?;
    timeout(DEFAULT_READ_TIMEOUT, mcp.initialize()).await??;

    let request_id = mcp
        .send_thread_set_tool_enabled_request(ThreadSetToolEnabledParams {
            thread_id: "00000000-0000-0000-0000-000000000000".to_string(),
            name: "apply_patch".to_string(),
            enabled: false,
        })
        .await?;
    timeout(
        DEFAULT_READ_TIMEOUT,
        mcp.read_stream_until_error_message(RequestId::Integer(request_id)),
    )
    .await??;

    Ok(())
}
