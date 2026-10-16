use loom_ipc::{IpcHandler, IpcRequest, IpcResponse};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Wraps an IPC handler and records activity timestamps for idle shutdown.
pub struct ActivityHandler<H> {
    inner: H,
    last_activity: Arc<AtomicU64>,
}

impl<H> ActivityHandler<H> {
    pub fn new(inner: H, last_activity: Arc<AtomicU64>) -> Self {
        last_activity.store(now_ms(), Ordering::Relaxed);
        Self {
            inner,
            last_activity,
        }
    }
}

impl<H: IpcHandler> IpcHandler for ActivityHandler<H> {
    fn handle(&self, req: IpcRequest) -> IpcResponse {
        self.last_activity.store(now_ms(), Ordering::Relaxed);
        self.inner.handle(req)
    }
}
