//! GenericAdapter tests: TOML parsing, trait auto-implementation, scanner, health.

use std::fs;

use loom_core::HealthCheck;
use loom_generic_adapter::config::ToolToml;
use loom_generic_adapter::scanner::AdapterDirScanner;
use loom_generic_adapter::GenericAdapter;

// ─── Sample TOML fixtures ───────────────────────────────────────────

const FULL_TOML: &str = r#"
[tool]
name = "cursor"
version = "0.45.0"

[skills]
native_dir = "~/.cursor/skills/"
import_on_register = true

[mcp]
config_path = "~/.cursor/mcp.json"
shared_servers = ["filesystem", "github"]

[auth]
store_path = "~/.cursor/auth.json"
providers = ["anthropic", "openai"]

[hooks]
scripts_dir = "~/.cursor/hooks/"

[routing]
default_tier = "mid"

[sessions]
store_path = "~/.cursor/sessions/"
auto_export = true
"#;

const MINIMAL_TOML: &str = r#"
[tool]
name = "minimal-tool"
version = "1.0.0"
"#;

const SKILLS_ONLY_TOML: &str = r#"
[tool]
name = "skill-only"
version = "1.0.0"

[skills]
native_dir = "~/.test/skills/"
import_on_register = true
"#;

const EMPTY_NAME_TOML: &str = r#"
[tool]
name = ""
version = "1.0.0"
"#;

// ─── TOML parsing tests ─────────────────────────────────────────────

#[test]
fn test_parse_full_toml() {
    let config: ToolToml = toml::from_str(FULL_TOML).unwrap();
    assert_eq!(config.tool.name, "cursor");
    assert_eq!(config.tool.version, "0.45.0");
    assert!(config.skills.is_some());
    assert!(config.mcp.is_some());
    assert!(config.auth.is_some());
    assert!(config.hooks.is_some());
    assert!(config.routing.is_some());
    assert!(config.sessions.is_some());
}

#[test]
fn test_parse_minimal_toml() {
    let config: ToolToml = toml::from_str(MINIMAL_TOML).unwrap();
    assert_eq!(config.tool.name, "minimal-tool");
    assert_eq!(config.tool.version, "1.0.0");
    assert!(config.skills.is_none());
    assert!(config.mcp.is_none());
    assert!(config.auth.is_none());
    assert!(config.hooks.is_none());
    assert!(config.routing.is_none());
    assert!(config.sessions.is_none());
}

#[test]
fn test_parse_skills_section() {
    let config: ToolToml = toml::from_str(SKILLS_ONLY_TOML).unwrap();
    let skills = config.skills.unwrap();
    assert_eq!(skills.native_dir, Some("~/.test/skills/".to_string()));
    assert!(skills.import_on_register);
}

#[test]
fn test_parse_mcp_section() {
    let config: ToolToml = toml::from_str(FULL_TOML).unwrap();
    let mcp = config.mcp.unwrap();
    assert_eq!(mcp.shared_servers, vec!["filesystem", "github"]);
    assert_eq!(mcp.config_path, Some("~/.cursor/mcp.json".to_string()));
}

#[test]
fn test_parse_auth_section() {
    let config: ToolToml = toml::from_str(FULL_TOML).unwrap();
    let auth = config.auth.unwrap();
    assert_eq!(auth.providers, vec!["anthropic", "openai"]);
}

#[test]
fn test_parse_routing_section() {
    let config: ToolToml = toml::from_str(FULL_TOML).unwrap();
    let routing = config.routing.unwrap();
    assert_eq!(routing.default_tier, "mid");
}

#[test]
fn test_parse_sessions_section() {
    let config: ToolToml = toml::from_str(FULL_TOML).unwrap();
    let sessions = config.sessions.unwrap();
    assert!(sessions.auto_export);
}

// ─── GenericAdapter construction tests ──────────────────────────────

#[test]
fn test_adapter_from_toml_full() {
    let adapter = GenericAdapter::from_toml("/tmp/.agents", "cursor", FULL_TOML).unwrap();
    assert_eq!(adapter.tool_name(), "cursor");
    assert_eq!(adapter.tool_version(), "0.45.0");

    let ts = adapter.build();
    assert!(ts.has_skills());
    assert!(ts.has_mcp());
    assert!(ts.has_auth());
    assert!(ts.has_hooks());
    assert!(ts.has_routing());
    assert!(ts.has_sessions());
    assert_eq!(ts.present_features().len(), 6);
    assert!(ts.absent_features().is_empty());
}

#[test]
fn test_adapter_from_toml_minimal() {
    let adapter = GenericAdapter::from_toml("/tmp/.agents", "minimal", MINIMAL_TOML).unwrap();
    assert_eq!(adapter.tool_name(), "minimal");

    let ts = adapter.build();
    assert!(!ts.has_skills());
    assert!(!ts.has_mcp());
    assert!(!ts.has_auth());
    assert!(!ts.has_hooks());
    assert!(!ts.has_routing());
    assert!(!ts.has_sessions());
    assert!(ts.present_features().is_empty());
    assert_eq!(ts.absent_features().len(), 6);
}

#[test]
fn test_adapter_from_toml_skills_only() {
    let adapter =
        GenericAdapter::from_toml("/tmp/.agents", "skill-only", SKILLS_ONLY_TOML).unwrap();

    let ts = adapter.build();
    assert!(ts.has_skills());
    assert!(!ts.has_mcp());
    assert!(!ts.has_auth());
    assert!(!ts.has_hooks());
    assert!(!ts.has_routing());
    assert!(!ts.has_sessions());
}

#[test]
fn test_adapter_invalid_toml() {
    let result = GenericAdapter::from_toml("/tmp/.agents", "bad", "not valid toml [[[");
    assert!(result.is_err());
}

#[test]
fn test_adapter_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    let result = GenericAdapter::new(tmp.path().to_str().unwrap(), "nonexistent");
    assert!(result.is_err());
}

// ─── Scanner tests ──────────────────────────────────────────────────

#[test]
fn test_scanner_empty_directory() {
    let tmp = tempfile::tempdir().unwrap();
    let registrations = AdapterDirScanner::scan(tmp.path().to_str().unwrap());
    assert!(registrations.is_empty());
}

#[test]
fn test_scanner_no_tools_dir() {
    // A directory that exists but has no tools subdir
    let tmp = tempfile::tempdir().unwrap();
    let registrations = AdapterDirScanner::scan(tmp.path().to_str().unwrap());
    assert!(registrations.is_empty());
}

#[test]
fn test_scanner_count_zero() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(AdapterDirScanner::count(tmp.path().to_str().unwrap()), 0);
}

#[test]
fn test_scanner_single_file() {
    let tmp = tempfile::tempdir().unwrap();
    let tools_dir = tmp.path().join("tools");
    fs::create_dir_all(&tools_dir).unwrap();
    fs::write(tools_dir.join("cursor.toml"), FULL_TOML).unwrap();

    let registrations = AdapterDirScanner::scan(tmp.path().to_str().unwrap());
    assert_eq!(registrations.len(), 1);
    assert_eq!(registrations[0].name, "cursor");
    assert!(registrations[0].warnings.is_empty());

    assert_eq!(AdapterDirScanner::count(tmp.path().to_str().unwrap()), 1);
}

#[test]
fn test_scanner_multiple_files() {
    let tmp = tempfile::tempdir().unwrap();
    let tools_dir = tmp.path().join("tools");
    fs::create_dir_all(&tools_dir).unwrap();
    fs::write(tools_dir.join("cursor.toml"), FULL_TOML).unwrap();
    fs::write(tools_dir.join("jcode.toml"), MINIMAL_TOML).unwrap();
    fs::write(tools_dir.join("skill-only.toml"), SKILLS_ONLY_TOML).unwrap();

    let registrations = AdapterDirScanner::scan(tmp.path().to_str().unwrap());
    assert_eq!(registrations.len(), 3);

    let names: Vec<&str> = registrations.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"cursor"));
    assert!(names.contains(&"minimal-tool"));
    assert!(names.contains(&"skill-only"));
}

#[test]
fn test_scanner_skips_non_toml() {
    let tmp = tempfile::tempdir().unwrap();
    let tools_dir = tmp.path().join("tools");
    fs::create_dir_all(&tools_dir).unwrap();
    fs::write(tools_dir.join("notes.txt"), "not a toml").unwrap();
    fs::write(tools_dir.join("cursor.toml"), FULL_TOML).unwrap();

    assert_eq!(AdapterDirScanner::count(tmp.path().to_str().unwrap()), 1);
    let registrations = AdapterDirScanner::scan(tmp.path().to_str().unwrap());
    assert_eq!(registrations.len(), 1);
}

#[test]
fn test_scanner_empty_name_warning() {
    let tmp = tempfile::tempdir().unwrap();
    let tools_dir = tmp.path().join("tools");
    fs::create_dir_all(&tools_dir).unwrap();
    fs::write(tools_dir.join("empty.toml"), EMPTY_NAME_TOML).unwrap();

    let registrations = AdapterDirScanner::scan(tmp.path().to_str().unwrap());
    assert_eq!(registrations.len(), 1);
    assert!(!registrations[0].warnings.is_empty());
    assert!(registrations[0]
        .warnings
        .iter()
        .any(|w| w.contains("tool.name is empty")));
}

#[test]
fn test_scanner_parse_file() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("test.toml");
    fs::write(&path, FULL_TOML).unwrap();

    let reg = AdapterDirScanner::parse_file(&path).unwrap();
    assert_eq!(reg.name, "cursor");
    assert_eq!(reg.config.tool.version, "0.45.0");
}

// ─── Health check ───────────────────────────────────────────────────

#[test]
fn test_adapter_health() {
    let adapter = GenericAdapter::from_toml("/tmp/.agents", "health-test", FULL_TOML).unwrap();
    let health = adapter.health().expect("health should succeed");
    assert_eq!(health.crate_name, "loom-generic-adapter");
    assert!(!health.version.is_empty());
}

// ─── Edge cases ─────────────────────────────────────────────────────

#[test]
fn test_missing_sections_default_values() {
    let config: ToolToml = toml::from_str(MINIMAL_TOML).unwrap();
    assert_eq!(config.tool.name, "minimal-tool");
    // All optional fields should be None
    assert!(config.skills.is_none());
    assert!(config.mcp.is_none());
    assert!(config.auth.is_none());
    assert!(config.hooks.is_none());
    assert!(config.routing.is_none());
    assert!(config.sessions.is_none());
}

#[test]
fn test_mcp_section_defaults() {
    let toml = r#"
[tool]
name = "test"
version = "1.0.0"

[mcp]
"#;
    let config: ToolToml = toml::from_str(toml).unwrap();
    let mcp = config.mcp.unwrap();
    assert!(mcp.shared_servers.is_empty());
    assert!(!mcp.import_on_register);
    assert!(mcp.import_tags.is_empty());
}
