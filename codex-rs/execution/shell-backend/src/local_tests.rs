use std::collections::HashMap;

use tokio::io::AsyncReadExt;

use codex_path::absolute_path::AbsolutePathBuf;
use codex_protocol::permissions::NetworkSandboxPolicy;

use super::*;
use crate::StdioPolicy::RedirectForShellTool;

#[cfg(unix)]
fn shell_command(script: &str) -> Vec<String> {
    vec!["/bin/sh".to_string(), "-c".to_string(), script.to_string()]
}

#[cfg(unix)]
fn spawn_request(script: &str) -> SpawnShellRequest {
    SpawnShellRequest {
        program: shell_command(script)
            .first()
            .cloned()
            .expect("command always non-empty")
            .into(),
        args: shell_command(script).into_iter().skip(1).collect(),
        arg0: None,
        cwd: AbsolutePathBuf::current_dir().expect("cwd is available in tests"),
        network_sandbox_policy: NetworkSandboxPolicy::Enabled,
        network: None,
        stdio_policy: RedirectForShellTool,
        env: HashMap::from([(
            "PATH".to_string(),
            std::env::var("PATH").unwrap_or_default(),
        )]),
    }
}

#[cfg(unix)]
#[tokio::test]
async fn spawn_streams_piped_stdout_and_exits_successfully() {
    let backend = StdSystemProcessBackend;
    let mut process = backend
        .spawn(spawn_request("printf hello"))
        .await
        .expect("spawn should succeed");

    let mut stdout = process
        .take_stdout()
        .expect("RedirectForShellTool pipes stdout");
    assert!(process.take_stderr().is_some());

    let mut output = String::new();
    stdout
        .read_to_string(&mut output)
        .await
        .expect("read stdout");

    let status = process.wait().await.expect("wait");
    assert!(status.success());
    assert_eq!(output, "hello");
}

#[cfg(unix)]
#[tokio::test]
async fn kill_process_group_terminates_descendants() {
    let backend = StdSystemProcessBackend;
    let mut process = backend
        .spawn(spawn_request("sleep 30 & echo $!; sleep 30"))
        .await
        .expect("spawn should succeed");

    let mut stdout = process
        .take_stdout()
        .expect("RedirectForShellTool pipes stdout");
    let mut pid_line = String::new();
    // The process keeps its stdout pipe open, so read exactly one byte at a
    // time until the newline that follows the descendant pid.
    let mut byte = [0u8; 1];
    while !pid_line.ends_with('\n') {
        let n = stdout.read(&mut byte).await.expect("read pid line");
        assert!(n > 0, "stdout closed before a pid was printed");
        pid_line.push(byte[0] as char);
    }
    assert!(pid_line.trim().parse::<u32>().is_ok());

    process
        .kill_process_group()
        .expect("kill process group should succeed");
    process.start_kill().expect("start kill should succeed");

    let status = process.wait().await.expect("wait after kill");
    assert!(!status.success());
}
