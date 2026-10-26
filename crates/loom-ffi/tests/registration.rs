//! FFI registration tests: tool round-trip, skill round-trip, name
//! validation, duplicate behavior, panic containment. All tempfile-based
//! (repo convention); the native side is exercised directly plus one test
//! through the actual `loom_register_tool` export.

use loom_generic_adapter::config::ToolToml;
use loom_ffi::registration;
use serde_json::{json, Value};

const GOOD_SKILL: &str = "---\nname: myskill\ndescription: Use when testing skills.\n---\n\nBody.\n";

fn assert_ok(v: &Value) {
    assert_eq!(v["ok"], json!(true), "expected ok:true, got {v}");
}

fn assert_err(v: &Value, needle: &str) {
    assert_eq!(v["ok"], json!(false), "expected ok:false, got {v}");
    assert!(
        v["error"].as_str().unwrap().contains(needle),
        "error '{}' should mention '{needle}'",
        v["error"]
    );
}

#[test]
fn tool_round_trip_and_duplicate_is_updated() {
    let dir = tempfile::tempdir().unwrap();
    let loom_dir = dir.path();

    let v = registration::register_tool(loom_dir, r#"{"name":"mytool"}"#);
    assert_ok(&v);
    assert_eq!(v["updated"], json!(false));
    let path = dir.path().join("tools").join("mytool.toml");
    assert_eq!(v["toml_path"], json!(path.to_string_lossy()));
    let toml: ToolToml = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(toml.tool.name, "mytool");
    assert!(toml.launch.is_none());

    let v = registration::register_tool(
        loom_dir,
        r#"{"name":"mytool","command":"/usr/bin/mytool","args":["--x"],"env":{"Y":"1"}}"#,
    );
    assert_ok(&v);
    assert_eq!(v["updated"], json!(true));
    let toml: ToolToml = toml::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let launch = toml.launch.unwrap();
    assert_eq!(launch.command, "/usr/bin/mytool");
    assert_eq!(launch.args, vec!["--x".to_string()]);
    assert_eq!(launch.env.get("Y").map(String::as_str), Some("1"));
}

#[test]
fn unregister_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let loom_dir = dir.path();
    registration::register_tool(loom_dir, r#"{"name":"mytool"}"#);
    let path = dir.path().join("tools").join("mytool.toml");
    assert!(path.exists());

    assert_ok(&registration::unregister_tool(loom_dir, "mytool"));
    assert!(!path.exists());

    assert_err(
        &registration::unregister_tool(loom_dir, "mytool"),
        "not found",
    );
}

#[test]
fn tool_name_validation() {
    let dir = tempfile::tempdir().unwrap();
    let loom_dir = dir.path();
    for name in ["", ".", "..", "a/b", "a\\b", "../evil"] {
        assert_err(
            &registration::register_tool(loom_dir, &json!({ "name": name }).to_string()),
            "invalid tool name",
        );
    }
    assert_err(
        &registration::register_tool(loom_dir, r#"{"name":"../evil"}"#),
        "invalid tool name",
    );
    assert_err(
        &registration::register_tool(loom_dir, r#"{}"#),
        "missing required field",
    );
    assert_err(
        &registration::register_tool(loom_dir, r#"{"name":"t","command":"c","args":"nope"}"#),
        "'args' must be an array of strings",
    );
    assert_err(
        &registration::register_tool(loom_dir, "not json"),
        "invalid JSON payload",
    );
}

#[test]
fn skill_round_trip_and_duplicate_refused() {
    let dir = tempfile::tempdir().unwrap();
    let loom_dir = dir.path();

    // Frontmatter `name:` wins, like `loom skills install`.
    let v = registration::register_skill_from_json(
        loom_dir,
        &json!({ "name": "fallback", "content": GOOD_SKILL }).to_string(),
);
    assert_ok(&v);
    assert_eq!(v["name"], json!("myskill"));
    assert!(dir.path().join("skills").join("myskill.md").is_file());

    // No payload name: frontmatter still applied, temp stem only a fallback.
    let v = registration::register_skill_from_json(
        loom_dir,
        &json!({ "content": GOOD_SKILL.replace("myskill", "other") }).to_string(),
);
    assert_ok(&v);
    assert_eq!(v["name"], json!("other"));

    // Duplicate refused, matching `loom skills install`.
    assert_err(
        &registration::register_skill_from_json(
            loom_dir,
            &json!({ "name": "myskill", "content": GOOD_SKILL }).to_string(),
        ),
        "already exists",
    );
}

#[cfg(test)]
mod ffi_boundary_tests {
    use super::*;

    fn as_json(s: *mut std::ffi::CString) -> Value {
        let owned: &std::ffi::CString = unsafe { &*s };
        let text = owned.to_str().unwrap().to_string();
        unsafe { loom_ffi::loom_free_string(s) };
        serde_json::from_str(&text).unwrap()
    }

    /// End-to-end through the C-ABI export with `LOOM_AGENTS_DIR` pointed at
    /// a tempdir; the pointer is freed through `loom_free_string`.
    #[test]
    fn exported_loom_register_tool_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        unsafe { std::env::set_var("LOOM_AGENTS_DIR", dir.path()) };
        let payload = std::ffi::CString::new(r#"{"name":"ffitool"}"#).unwrap();
        let out = unsafe { loom_ffi::loom_register_tool(payload.as_ptr()) };
        assert_ok(&as_json(out));
        assert!(dir.path().join("tools").join("ffitool.toml").is_file());

        let name = std::ffi::CString::new("ffitool").unwrap();
        let out = unsafe { loom_ffi::loom_unregister_tool(name.as_ptr()) };
        assert_ok(&as_json(out));
        assert!(!dir.path().join("tools").join("ffitool.toml").exists());
        unsafe { std::env::remove_var("LOOM_AGENTS_DIR") };
    }

    #[test]
    fn null_payload_is_structured_error() {
        let out = unsafe { loom_ffi::loom_register_tool(std::ptr::null()) };
        assert_err(&as_json(out), "null");
    }

    #[test]
    fn invalid_utf8_payload_is_structured_error() {
        let bad: &[u8] = b"[\xff]\0";
        let out = unsafe { loom_ffi::loom_register_tool(bad.as_ptr().cast()) };
        assert_err(&as_json(out), "invalid UTF-8");
    }
}
