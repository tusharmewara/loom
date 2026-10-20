use std::time::Duration;

use loom_ipc::{send_command_with_timeout, IpcRequest};
use tokio::net::UnixListener;

/// A daemon that accepts the connection but never replies must not hang the caller.
#[tokio::test]
async fn send_command_times_out_when_daemon_never_replies() {
    let dir = tempfile::tempdir().expect("tempdir");
    let sock = dir.path().join("loomd.sock");
    let sock_path = sock.to_str().expect("utf8 path");

    let listener = UnixListener::bind(&sock).expect("bind");
    // Accept the connection, then hold it open without replying.
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.expect("accept");
        // Keep the stream alive without writing anything.
        let _stream = stream;
        tokio::time::sleep(Duration::from_secs(30)).await;
    });

    let err = send_command_with_timeout(
        sock_path,
        IpcRequest::new("health.check"),
        Duration::from_millis(200),
    )
    .await
    .expect_err("expected a timeout error");

    let msg = format!("{}", err);
    assert!(
        msg.to_lowercase().contains("timed out"),
        "error should mention a timeout, got: {}",
        msg
    );
    assert!(
        msg.contains(sock_path),
        "error should name the socket path, got: {}",
        msg
    );
}
