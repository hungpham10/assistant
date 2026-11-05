#![cfg(unix)]
#![allow(clippy::unwrap_used)]

//! Skills mentioned in turn input may declare MCP dependencies that are not
//! installed yet. The dependency install + MCP runtime refresh must happen
//! before the first step context is captured, so the first sampling request
//! already advertises the dependency server's tools.

use std::net::TcpListener;
use std::time::Duration;
use std::time::Instant;

use anyhow::Result;
use codex_exec_server::CreateDirectoryOptions;
use codex_exec_server::ExecutorFileSystem;
use codex_path::absolute_path::AbsolutePathBuf;
use codex_path::path_uri::PathUri;
use codex_protocol::models::PermissionProfile;
use codex_protocol::protocol::AskForApproval;
use codex_protocol::protocol::Op;
use codex_protocol::user_input::UserInput;
use core_test_support::responses::ev_assistant_message;
use core_test_support::responses::ev_completed;
use core_test_support::responses::ev_response_created;
use core_test_support::responses::mount_sse_once;
use core_test_support::responses::sse;
use core_test_support::responses::start_mock_server;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::test_codex::turn_permission_fields;
use core_test_support::wait_for_event;
use tokio::net::TcpStream;
use tokio::process::Child;
use tokio::process::Command;
use tokio::time::sleep;

/// Starts the shared streamable HTTP MCP test server binary and returns its
/// endpoint URL plus the running child process.
async fn start_dep_mcp_server() -> Result<Option<(String, Child)>> {
    let bin = match codex_utils_cargo_bin::cargo_bin("test_streamable_http_server") {
        Ok(path) => path,
        Err(err) => {
            eprintln!("test_streamable_http_server binary not available, skipping test: {err}");
            return Ok(None);
        }
    };
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let bind_addr = format!("127.0.0.1:{port}");
    let server_url = format!("http://{bind_addr}/mcp");

    let mut child = Command::new(&bin)
        .kill_on_drop(true)
        .env("MCP_STREAMABLE_HTTP_BIND_ADDR", &bind_addr)
        .env("MCP_TEST_VALUE", "dependency-test")
        .spawn()?;

    wait_for_mcp_server(&mut child, &bind_addr, &server_url, Duration::from_secs(5)).await?;
    Ok(Some((server_url, child)))
}

async fn wait_for_mcp_server(
    child: &mut Child,
    bind_addr: &str,
    server_url: &str,
    timeout: Duration,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            anyhow::bail!("MCP test server exited early with status {status}");
        }
        if Instant::now() >= deadline {
            anyhow::bail!("timed out waiting for MCP test server at {server_url}");
        }
        if tokio::time::timeout(Duration::from_millis(200), TcpStream::connect(bind_addr))
            .await
            .is_ok_and(|result| result.is_ok())
        {
            return Ok(());
        }
        sleep(Duration::from_millis(50)).await;
    }
}

async fn write_skill_with_mcp_dependency(
    cwd: AbsolutePathBuf,
    fs: std::sync::Arc<dyn ExecutorFileSystem>,
    skill_name: &str,
    dependency_url: &str,
) -> Result<()> {
    let skill_dir = cwd.join(".agents").join("skills").join(skill_name);
    let skill_dir_uri = PathUri::from_host_native_path(&skill_dir)?;
    fs.create_directory(
        &skill_dir_uri,
        CreateDirectoryOptions { recursive: true },
        /*sandbox*/ None,
    )
    .await?;
    let contents =
        format!("---\nname: {skill_name}\ndescription: {skill_name} skill\n---\n\nskill body\n");
    let path = skill_dir.join("SKILL.md");
    let path_uri = PathUri::from_host_native_path(&path)?;
    fs.write_file(&path_uri, contents.into_bytes(), /*sandbox*/ None)
        .await?;

    // Skill MCP dependencies are declared in the `agents/openai.yaml` metadata
    // file, not the SKILL.md frontmatter.
    let metadata_dir = skill_dir.join("agents");
    let metadata_dir_uri = PathUri::from_host_native_path(&metadata_dir)?;
    fs.create_directory(
        &metadata_dir_uri,
        CreateDirectoryOptions { recursive: true },
        /*sandbox*/ None,
    )
    .await?;
    let metadata = format!(
        "dependencies:\n  tools:\n    - type: mcp\n      value: dep-server\n      transport: streamable_http\n      url: {dependency_url}\n"
    );
    let metadata_path = metadata_dir.join("openai.yaml");
    let metadata_uri = PathUri::from_host_native_path(&metadata_path)?;
    fs.write_file(&metadata_uri, metadata.into_bytes(), /*sandbox*/ None)
        .await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn skill_mcp_dependency_tools_advertised_in_first_request() -> Result<()> {
    skip_if_no_network!(Ok(()));

    let Some((server_url, _mcp_server)) = start_dep_mcp_server().await? else {
        return Ok(());
    };

    let server = start_mock_server().await;
    let mcp_url = server_url.clone();
    let mut builder = test_codex().with_workspace_setup(move |cwd, fs| async move {
        write_skill_with_mcp_dependency(cwd, fs, "dep-skill", &mcp_url).await
    });
    let test = builder.build_with_auto_env(&server).await?;

    let mock = mount_sse_once(
        &server,
        sse(vec![
            ev_response_created("resp-1"),
            ev_assistant_message("msg-1", "done"),
            ev_completed("resp-1"),
        ]),
    )
    .await;

    let session_model = test.session_configured.model.clone();
    let environment_selection = test.executor_environment().selection().clone();
    let (sandbox_policy, permission_profile) =
        turn_permission_fields(PermissionProfile::Disabled, test.config.cwd.as_path());
    test.codex
        .submit(Op::UserInput {
            items: vec![UserInput::Text {
                text: "please use $dep-skill".to_string(),
                text_elements: Vec::new(),
            }],
            final_output_json_schema: None,
            responsesapi_client_metadata: None,
            additional_context: Default::default(),
            thread_settings: codex_protocol::protocol::ThreadSettingsOverrides {
                environments: Some(codex_protocol::protocol::TurnEnvironmentSelections::new(
                    test.config.cwd.clone(),
                    vec![environment_selection],
                )),
                approval_policy: Some(AskForApproval::Never),
                sandbox_policy: Some(sandbox_policy),
                permission_profile,
                collaboration_mode: Some(codex_protocol::config_types::CollaborationMode {
                    mode: codex_protocol::config_types::ModeKind::Default,
                    settings: codex_protocol::config_types::Settings {
                        model: session_model,
                        reasoning_effort: None,
                        developer_instructions: None,
                    },
                }),
                ..Default::default()
            },
        })
        .await?;

    wait_for_event(test.codex.as_ref(), |event| {
        matches!(event, codex_protocol::protocol::EventMsg::TurnComplete(_))
    })
    .await;

    // The dependency server should have been persisted to config.
    let config_entries: Vec<String> = std::fs::read_dir(test.codex_home_path())?
        .filter_map(|entry| {
            entry
                .ok()
                .map(|e| e.file_name().to_string_lossy().into_owned())
        })
        .collect();
    eprintln!("codex_home entries: {config_entries:?}");
    let config_toml = std::fs::read_to_string(test.codex_home_path().join("config.toml"))?;
    assert!(
        config_toml.contains(&server_url),
        "dependency server missing from config.toml:\n{config_toml}"
    );

    // The FIRST sampling request must already advertise the dependency
    // server; before the fix it was built from a stale MCP snapshot. MCP
    // tools may be advertised directly as function tools or deferred behind
    // `tool_search`, so match on the server name anywhere in the tools array.
    let request = mock.single_request();
    let body = request.body_json();
    let tools: Vec<serde_json::Value> = body
        .get("tools")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    let tools_json = serde_json::to_string(&tools)?;
    assert!(
        tools_json.contains("dep-server"),
        "first sampling request should advertise the dependency server, got: {tools_json}"
    );

    Ok(())
}
