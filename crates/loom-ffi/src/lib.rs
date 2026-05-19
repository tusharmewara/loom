pub mod bindings;

use loom_core::HealthCheck;

pub struct LoomFfi;

impl LoomFfi {
    pub fn new() -> Self {
        Self {}
    }
}

impl HealthCheck for LoomFfi {
    fn health(&self) -> anyhow::Result<loom_core::HealthStatus> {
        Ok(loom_core::HealthStatus {
            crate_name: "loom-ffi",
            version: env!("CARGO_PKG_VERSION"),
            uptime_seconds: 0,
            last_error: None,
            performance_counters: std::collections::HashMap::new(),
        })
    }
}

// ─── C FFI exports ────────────────────────────────────────────────

#[no_mangle]
pub extern "C" fn loom_version() -> *const u8 {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr()
}

#[no_mangle]
pub extern "C" fn loom_health_json() -> *mut std::ffi::CString {
    let status = LoomFfi::new().health().unwrap();
    let json = serde_json::to_string(&status).unwrap();
    Box::into_raw(Box::new(std::ffi::CString::new(json).unwrap()))
}

#[no_mangle]
pub extern "C" fn loom_free_string(s: *mut std::ffi::CString) {
    if !s.is_null() {
        unsafe { drop(Box::from_raw(s)); }
    }
}
