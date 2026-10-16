pub mod bindings;

use loom_core::HealthCheck;

pub struct LoomFfi;

impl Default for LoomFfi {
    fn default() -> Self {
        Self::new()
    }
}

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

/// Frees a string allocated by `loom_get_version`.
///
/// # Safety
///
/// The pointer `s` must have been allocated by `loom_get_version` and must not
/// be used after this function is called. Passing a null pointer is safe.
#[no_mangle]
pub unsafe extern "C" fn loom_free_string(s: *mut std::ffi::CString) {
    if !s.is_null() {
        unsafe {
            drop(Box::from_raw(s));
        }
    }
}

// ─── Routing C FFI (not available on WASM) ─────────────────────────

#[cfg(not(target_arch = "wasm32"))]
mod routing_ffi {
    use loom_routing::RoutingEngine;

    /// Classify a prompt and return the complexity score as a JSON string.
    /// Caller must free the result with `loom_free_string`.
    #[no_mangle]
    pub extern "C" fn loom_classify_prompt(
        prompt: *const std::ffi::c_char,
    ) -> *mut std::ffi::CString {
        let prompt_str = unsafe {
            if prompt.is_null() {
                return Box::into_raw(Box::new(
                    std::ffi::CString::new(r#"{"error":"null prompt"}"#).unwrap(),
                ));
            }
            match std::ffi::CStr::from_ptr(prompt).to_str() {
                Ok(s) => s,
                Err(_) => {
                    return Box::into_raw(Box::new(
                        std::ffi::CString::new(r#"{"error":"invalid utf-8"}"#).unwrap(),
                    ));
                }
            }
        };

        let engine = RoutingEngine::new("");
        let score = engine.classify(prompt_str);
        let json = serde_json::to_string(&score).unwrap_or_default();
        Box::into_raw(Box::new(std::ffi::CString::new(json).unwrap()))
    }

    /// Resolve a routing decision for a prompt, returning the decision as a JSON string.
    /// Caller must free the result with `loom_free_string`.
    #[no_mangle]
    pub extern "C" fn loom_resolve_route(
        prompt: *const std::ffi::c_char,
    ) -> *mut std::ffi::CString {
        let prompt_str = unsafe {
            if prompt.is_null() {
                return Box::into_raw(Box::new(
                    std::ffi::CString::new(r#"{"error":"null prompt"}"#).unwrap(),
                ));
            }
            match std::ffi::CStr::from_ptr(prompt).to_str() {
                Ok(s) => s,
                Err(_) => {
                    return Box::into_raw(Box::new(
                        std::ffi::CString::new(r#"{"error":"invalid utf-8"}"#).unwrap(),
                    ));
                }
            }
        };

        let engine = RoutingEngine::new("");
        let score = engine.classify(prompt_str);
        let decision = engine.decide(score);
        let json = serde_json::to_string(&decision).unwrap_or_default();
        Box::into_raw(Box::new(std::ffi::CString::new(json).unwrap()))
    }
}
