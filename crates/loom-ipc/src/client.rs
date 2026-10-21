use crate::protocol::{IpcRequest, IpcResponse};
use anyhow::{Context, Result};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::time::timeout;

/// Default deadline for a whole IPC request/response round trip.
pub const DEFAULT_REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// How many times a client connects before it gives up.
pub const CONNECT_ATTEMPTS: u32 = 8;

/// The first backoff interval between connect attempts; it doubles after
/// every failed attempt, up to [`CONNECT_BACKOFF_MAX`].
pub const CONNECT_BACKOFF_BASE: Duration = Duration::from_millis(200);

/// The ceiling on one backoff interval.
///
/// The intervals stop growing here so the total wait stays bounded: a cold
/// daemon needs a couple of seconds to bind its socket, and the caller
/// should not be left waiting for a minute.
pub const CONNECT_BACKOFF_MAX: Duration = Duration::from_secs(1);

/// How long one connect attempt may take before it is abandoned.
///
/// Connecting to a Unix socket whose backlog is full blocks instead of
/// failing fast, so a wedged daemon must not hang the caller.
pub const CONNECT_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(1);

/// No daemon answered on the socket.
///
/// Returned when every connect attempt failed, so the caller can tell "the
/// daemon is not running" apart from "the daemon answered badly".
#[derive(Debug)]
pub struct ConnectError {
    /// The socket that was probed.
    pub socket_path: String,
    /// How many connect attempts were made.
    pub attempts: u32,
    /// Why the last attempt failed.
    pub last_error: String,
}

impl std::fmt::Display for ConnectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "no daemon answered on {} after {} attempt{}: {}",
            self.socket_path,
            self.attempts,
            if self.attempts == 1 { "" } else { "s" },
            self.last_error
        )
    }
}

impl std::error::Error for ConnectError {}

/// Connect to the daemon socket, retrying with a growing interval.
///
/// A live daemon is reached on the first attempt: one connect, no sleep. A
/// dead one costs `attempts` connects and a bounded wait, then a
/// [`ConnectError`] the caller can turn into advice.
pub async fn connect_with_backoff(
    socket_path: &str,
    attempts: u32,
    base_delay: Duration,
) -> Result<UnixStream, ConnectError> {
    let attempts = attempts.max(1);
    let mut delay = base_delay;
    let mut last_error = "no attempt was made".to_string();

    for attempt in 1..=attempts {
        match timeout(CONNECT_ATTEMPT_TIMEOUT, UnixStream::connect(socket_path)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(e)) => last_error = e.to_string(),
            Err(_) => last_error = format!("connect timed out after {:?}", CONNECT_ATTEMPT_TIMEOUT),
        }
        if attempt < attempts {
            tokio::time::sleep(delay).await;
            delay = delay.saturating_mul(2).min(CONNECT_BACKOFF_MAX);
        }
    }

    Err(ConnectError {
        socket_path: socket_path.to_string(),
        attempts,
        last_error,
    })
}

/// Send one request on an established connection and read the response.
pub async fn send_on_stream(
    stream: UnixStream,
    socket_path: &str,
    req: IpcRequest,
) -> Result<IpcResponse> {
    timeout(DEFAULT_REQUEST_TIMEOUT, exchange(stream, socket_path, req))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "IPC request timed out after {:?}: daemon did not respond on socket {}",
                DEFAULT_REQUEST_TIMEOUT,
                socket_path
            )
        })?
}

/// Connect to the loomd IPC socket and send a command, returning the response.
///
/// Uses [`DEFAULT_REQUEST_TIMEOUT`] as the overall deadline; a daemon that accepts
/// the connection but never replies can no longer hang the caller forever.
pub async fn send_command(socket_path: &str, req: IpcRequest) -> Result<IpcResponse> {
    send_command_with_timeout(socket_path, req, DEFAULT_REQUEST_TIMEOUT).await
}

/// Same as [`send_command`] but with an explicit overall deadline.
pub async fn send_command_with_timeout(
    socket_path: &str,
    req: IpcRequest,
    request_timeout: Duration,
) -> Result<IpcResponse> {
    timeout(request_timeout, send_command_inner(socket_path, req))
        .await
        .map_err(|_| {
            anyhow::anyhow!(
                "IPC request timed out after {:?}: daemon did not respond on socket {}",
                request_timeout,
                socket_path
            )
        })?
}

async fn send_command_inner(socket_path: &str, req: IpcRequest) -> Result<IpcResponse> {
    let stream = UnixStream::connect(socket_path)
        .await
        .with_context(|| format!("failed to connect to IPC socket {}", socket_path))?;
    exchange(stream, socket_path, req).await
}

/// Send one request on an established connection and read the response.
async fn exchange(stream: UnixStream, socket_path: &str, req: IpcRequest) -> Result<IpcResponse> {
    let (reader, mut writer) = stream.into_split();

    // Send request
    let req_line = req.to_json_line();
    writer
        .write_all(req_line.as_bytes())
        .await
        .with_context(|| format!("failed to write IPC request to {}", socket_path))?;

    // Read response
    let mut buf_reader = BufReader::new(reader);
    let mut line = String::new();
    let n = buf_reader
        .read_line(&mut line)
        .await
        .with_context(|| format!("failed to read IPC response from {}", socket_path))?;
    if n == 0 {
        anyhow::bail!(
            "IPC daemon closed the connection on {} without responding",
            socket_path
        );
    }

    let resp: IpcResponse = serde_json::from_str(line.trim())
        .with_context(|| format!("invalid IPC response from {}", socket_path))?;
    Ok(resp)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A connect that always fails must be bounded by the policy.
    #[tokio::test]
    async fn connect_backoff_is_bounded() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("loomd.sock");
        let socket = socket.to_str().expect("utf-8 path");

        let err = connect_with_backoff(socket, 3, Duration::from_millis(20))
            .await
            .expect_err("nothing is listening on that socket");

        assert_eq!(err.attempts, 3, "the policy bounds the attempts");
        assert!(err.socket_path.contains("loomd.sock"), "{err}");
        assert!(!err.last_error.is_empty(), "{err}");
    }

    /// The default policy waits long enough for a cold daemon to bind.
    ///
    /// A daemon started from a cold binary needs a couple of seconds to reach
    /// its socket bind; a window shorter than that reports a healthy daemon
    /// as unreachable.
    #[test]
    fn the_default_backoff_covers_a_slow_daemon_start() {
        let mut delay = CONNECT_BACKOFF_BASE;
        let mut total = Duration::ZERO;
        for _ in 1..CONNECT_ATTEMPTS {
            total += delay;
            delay = delay.saturating_mul(2).min(CONNECT_BACKOFF_MAX);
        }
        assert!(
            total >= Duration::from_secs(3),
            "the backoff window is {total:?}, too short for a cold daemon"
        );
    }

    /// A live daemon is reached on the first attempt: one connect, no sleep.
    #[tokio::test]
    async fn connect_backoff_does_not_wait_for_a_live_daemon() {
        let dir = tempfile::tempdir().expect("temp dir");
        let socket = dir.path().join("loomd.sock");
        let socket = socket.to_str().expect("utf-8 path");
        let _listener = tokio::net::UnixListener::bind(socket).expect("bind");

        // A huge backoff: one retry would make this test take minutes.
        let started = std::time::Instant::now();
        let stream = connect_with_backoff(socket, 3, Duration::from_secs(30))
            .await
            .expect("the listener is live");
        let elapsed = started.elapsed();

        drop(stream);
        assert!(
            elapsed < Duration::from_secs(5),
            "a live daemon must be used immediately, took {elapsed:?}"
        );
    }
}
