use loom_ipc::{
    connect_with_backoff, send_on_stream, IpcRequest, CONNECT_ATTEMPTS, CONNECT_BACKOFF_BASE,
};
use std::path::Path;
use tokio::net::UnixStream;

/// The daemon's IPC socket, relative to the ecosystem directory.
const DAEMON_SOCKET: &str = "loomd.sock";

/// The advice a user gets when the daemon cannot be reached: the cause and
/// what to type next, in one line.
pub fn stale_socket_advice(detail: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "daemon socket is stale (loomd is not running) — run `loom daemon restart` ({detail})"
    )
}

/// Connect to the daemon, starting it first if its socket is missing.
///
/// A socket *file* can outlive the daemon that bound it (a crash), so the
/// file's presence says nothing about liveness — only a connect does. The
/// connect is retried with a growing interval, and when it finally fails the
/// error names the cause and what to type next.
pub async fn ensure_daemon(loom_dir: &Path) -> anyhow::Result<UnixStream> {
    let socket_path = loom_dir.join(DAEMON_SOCKET);
    let socket = socket_path.to_str().ok_or_else(|| {
        anyhow::anyhow!(
            "daemon socket path is not valid UTF-8: {}",
            socket_path.display()
        )
    })?;

    if socket_path.exists() {
        connect_or_advise(socket, false).await
    } else {
        crate::commands::daemon::start_daemon(loom_dir)?;
        connect_or_advise(socket, true).await
    }
}

/// Connect to the daemon, and on failure say what happened and what to do.
///
/// `started_now` says whether we just started loomd: a daemon that was
/// already there and stopped answering left a stale socket behind, while a
/// daemon we started never came up at all.
async fn connect_or_advise(socket: &str, started_now: bool) -> anyhow::Result<UnixStream> {
    connect_with_backoff(socket, CONNECT_ATTEMPTS, CONNECT_BACKOFF_BASE)
        .await
        .map_err(|e| {
            if started_now {
                anyhow::anyhow!("loomd did not come up — run `loom daemon restart` ({e})")
            } else {
                stale_socket_advice(&e.to_string())
            }
        })
}

pub async fn send_daemon_command(
    loom_dir: &Path,
    req: IpcRequest,
) -> anyhow::Result<loom_ipc::IpcResponse> {
    let stream = ensure_daemon(loom_dir).await?;
    let socket_path = loom_dir.join(DAEMON_SOCKET);
    send_on_stream(stream, socket_path.to_str().unwrap_or_default(), req).await
}

/// Probe the daemon on its socket: one attempt, no retry, no auto-start.
///
/// For status-like commands that report the daemon's state — they must not
/// wait out a backoff to learn that the daemon is not there.
pub async fn probe_daemon(
    loom_dir: &Path,
    req: IpcRequest,
) -> anyhow::Result<loom_ipc::IpcResponse> {
    let socket_path = loom_dir.join(DAEMON_SOCKET);
    loom_ipc::send_command(socket_path.to_str().unwrap_or_default(), req).await
}

/// Send IPC only when the daemon socket already exists (no auto-start).
pub async fn send_daemon_command_if_running(
    loom_dir: &Path,
    req: IpcRequest,
) -> Option<loom_ipc::IpcResponse> {
    let socket_path = loom_dir.join(DAEMON_SOCKET);
    if !socket_path.exists() {
        return None;
    }
    probe_daemon(loom_dir, req).await.ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    /// A daemon that answers on the socket, counting the connections it takes.
    struct FakeDaemon {
        connections: Arc<AtomicUsize>,
    }

    impl FakeDaemon {
        fn start(dir: &Path) -> Self {
            let socket = dir.join(DAEMON_SOCKET);
            let listener = tokio::net::UnixListener::bind(&socket).expect("bind fake daemon");
            let connections = Arc::new(AtomicUsize::new(0));

            let counter = connections.clone();
            tokio::spawn(async move {
                while let Ok((stream, _)) = listener.accept().await {
                    counter.fetch_add(1, Ordering::SeqCst);
                    tokio::spawn(async move {
                        use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
                        let (reader, mut writer) = stream.into_split();
                        let mut reader = tokio::io::BufReader::new(reader);
                        let mut line = String::new();
                        if reader.read_line(&mut line).await.is_ok() {
                            let _ = writer
                                .write_all(b"{\"request_id\":\"x\",\"status\":\"ok\"}\n")
                                .await;
                        }
                    });
                }
            });

            Self { connections }
        }

        fn connections(&self) -> usize {
            self.connections.load(Ordering::SeqCst)
        }
    }

    /// A live daemon is used immediately: one connect, no added latency.
    #[tokio::test]
    async fn a_live_daemon_is_used_without_a_retry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let daemon = FakeDaemon::start(dir.path());

        let started = Instant::now();
        let resp = send_daemon_command(dir.path(), IpcRequest::new("health.check"))
            .await
            .expect("the fake daemon answers");
        let elapsed = started.elapsed();

        assert_eq!(resp.status, "ok");
        assert_eq!(
            daemon.connections(),
            1,
            "a live daemon must be reached on the first connect"
        );
        assert!(
            elapsed < Duration::from_millis(500),
            "the happy path must not sleep, took {elapsed:?}"
        );
    }

    /// A socket file with no daemon behind it: named cause, named remedy.
    #[tokio::test]
    async fn a_stale_socket_says_what_to_do() {
        let dir = tempfile::tempdir().expect("temp dir");
        // A socket file left behind by a crashed daemon: the file is there,
        // nothing answers on it.
        let socket = dir.path().join(DAEMON_SOCKET);
        std::fs::write(&socket, b"stale").expect("leave a stale socket file");

        let err = send_daemon_command(dir.path(), IpcRequest::new("health.check"))
            .await
            .expect_err("no daemon answers on a stale socket");
        let msg = err.to_string();

        assert!(
            msg.contains("daemon socket is stale (loomd is not running)"),
            "the cause must be named: {msg}"
        );
        assert!(
            msg.contains("run `loom daemon restart`"),
            "the remedy must be in the message: {msg}"
        );
    }

    /// A daemon that never comes up: bounded attempts, then the final message.
    #[tokio::test]
    async fn a_daemon_that_never_comes_up_says_what_to_do() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join(DAEMON_SOCKET);
        let socket = socket.to_str().expect("utf-8 path");

        let started = Instant::now();
        let err = connect_or_advise(socket, true)
            .await
            .expect_err("no daemon ever answers");
        let elapsed = started.elapsed();
        let msg = err.to_string();

        assert!(
            msg.contains("loomd did not come up"),
            "the cause must be named: {msg}"
        );
        assert!(
            msg.contains("run `loom daemon restart`"),
            "the remedy must be in the message: {msg}"
        );
        assert!(
            msg.contains(&format!("after {} attempts", CONNECT_ATTEMPTS)),
            "the bounded attempt count must be reported: {msg}"
        );
        assert!(
            elapsed < Duration::from_secs(15),
            "the backoff must be bounded, took {elapsed:?}"
        );
    }
}
