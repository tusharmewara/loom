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

// ─── Registration C FFI (cross-language SDK story) ─────────────────
//
// Mirrors `loom thread` / `loom skills install` / the plugin manager's TOML
// removal so a foreign process registers through the same codepath as the
// CLI. Results are structured JSON on both paths: `{"ok":true,...}` or
// `{"ok":false,"error":"..."}`. The loom dir comes from
// `loom_core::loom_dir()` (`LOOM_AGENTS_DIR` override honored) — the same
// env var the CLI and daemon use.

#[cfg(not(target_arch = "wasm32"))]
pub mod registration;

#[cfg(not(target_arch = "wasm32"))]
mod registration_ffi {
    use super::registration;
    use serde_json::json;
    use std::ffi::{c_char, CStr, CString};
    use std::panic::{catch_unwind, AssertUnwindSafe};

    /// Read a C string, `None` on null or invalid UTF-8
    /// (matches `loom_classify_prompt`'s handling).
    unsafe fn read_payload(payload: *const c_char) -> Option<String> {
        if payload.is_null() {
            return None;
        }
        CStr::from_ptr(payload).to_str().ok().map(String::from)
    }

    /// The catch-unwind trampoline: `work` runs inside it, and a panic
    /// degrades to `{"ok":false,"error":...}` instead of unwinding across
    /// the FFI boundary.
    pub(crate) fn call(
        payload: *const c_char,
        default_panic_error: &'static str,
        work: impl FnOnce(&str) -> serde_json::Value,
    ) -> *mut CString {
        let result =
            catch_unwind(AssertUnwindSafe(|| {
                match unsafe { read_payload(payload) } {
                    Some(payload) => work(&payload),
                    None => json!({ "ok": false, "error": "null or invalid UTF-8 payload" }),
                }
            }))
            .unwrap_or_else(|_| json!({ "ok": false, "error": default_panic_error }));
        Box::into_raw(Box::new(CString::new(result.to_string()).unwrap()))
    }

    /// Register a tool: JSON `{"name", "version"?, "command"?, "args"?, "env"?}`
    /// → the exact TOML `loom thread` writes at `{loom_dir}/tools/{name}.toml`.
    /// Success: `{"ok":true,"toml_path":"...","updated":bool}`.
    /// Caller must free the result with `loom_free_string`.
    ///
    /// # Safety
    ///
    /// `json` must either be null or point to a NUL-terminated UTF-8 string.
    #[no_mangle]
    pub unsafe extern "C" fn loom_register_tool(json: *const c_char) -> *mut CString {
        call(
            json,
            "panic inside loom_register_tool",
            |payload| {
                registration::register_tool(&std::path::PathBuf::from(loom_core::loom_dir()), payload)
            },
        )
    }

    /// Register a skill: JSON `{"name"?, "content"}` → the full `loom skills
    /// install` flow. Success: `{"ok":true,"name":"..."}`; duplicating an
    /// existing skill is refused, matching the CLI.
    /// Caller must free the result with `loom_free_string`.
    ///
    /// # Safety
    ///
    /// `json` must either be null or point to a NUL-terminated UTF-8 string.
    #[no_mangle]
    pub unsafe extern "C" fn loom_register_skill(json: *const c_char) -> *mut CString {
        call(
            json,
            "panic inside loom_register_skill",
            |payload| {
                registration::register_skill_from_json(
                    &std::path::PathBuf::from(loom_core::loom_dir()),
                    payload,
                )
            },
        )
    }

    /// Remove `{loom_dir}/tools/{name}.toml`. Success:
    /// `{"ok":true,"removed":"path"}`; absent tool is an error.
    /// Caller must free the result with `loom_free_string`.
    ///
    /// # Safety
    ///
    /// `name` must either be null or point to a NUL-terminated UTF-8 string.
    #[no_mangle]
    pub unsafe extern "C" fn loom_unregister_tool(name: *const c_char) -> *mut CString {
        call(
            name,
            "panic inside loom_unregister_tool",
            |name| {
                registration::unregister_tool(&std::path::PathBuf::from(loom_core::loom_dir()), name)
            },
        )
    }
}

// Integration tests (and foreign bindgens) address the exports from the crate
// root; `#[no_mangle]` alone only affects symbol emission.
#[cfg(not(target_arch = "wasm32"))]
pub use registration_ffi::{loom_register_skill, loom_register_tool, loom_unregister_tool};

#[cfg(all(test, not(target_arch = "wasm32")))]
mod registration_ffi_tests {
    use super::registration_ffi::call;
    use crate::loom_free_string;
    use serde_json::json;
    use std::ffi::CString;

    #[test]
    fn panic_is_contained_as_structured_error() {
        let payload = CString::new("x").unwrap();
        let out = call(payload.as_ptr(), "panic trapped", |_| panic!("boom"));
        let v: serde_json::Value =
            serde_json::from_str(unsafe { (*out).to_str().unwrap() }).unwrap();
        assert_eq!(v, json!({ "ok": false, "error": "panic trapped" }));
        unsafe { loom_free_string(out) };
    }

    #[test]
    fn free_round_trip_of_call_result() {
        let payload = CString::new("x").unwrap();
        let out = call(payload.as_ptr(), "unused", |s| {
            json!({ "ok": true, "echo": s })
        });
        let v: serde_json::Value =
            serde_json::from_str(unsafe { (*out).to_str().unwrap() }).unwrap();
        assert_eq!(v["echo"], json!("x"));
        unsafe { loom_free_string(out) };
    }
}
