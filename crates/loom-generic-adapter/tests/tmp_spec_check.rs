//! TEMPORARY: throwaway verification of spec claims. Delete after use.

use loom_generic_adapter::config::ToolToml;
use loom_generic_adapter::GenericAdapter;

#[test]
fn spec_skills_section_omitting_native_dir() {
    let toml = r#"
[tool]
name = "x"
version = "1.0.0"

[skills]
import_on_register = true
"#;
    let r: Result<ToolToml, _> = toml::from_str(toml);
    println!("skills-missing-native_dir => {:?}", r);
    assert!(r.is_ok(), "Option<String> without default should still parse?");
}

#[test]
fn spec_mcp_section_omitting_config_path() {
    let toml = r#"
[tool]
name = "x"
version = "1.0.0"

[mcp]
shared_servers = ["a"]
"#;
    let r: Result<ToolToml, _> = toml::from_str(toml);
    println!("mcp-missing-config_path => {:?}", r);
    assert!(r.is_ok());
}

#[test]
fn spec_unknown_key_is_ignored() {
    let toml = r#"
[tool]
name = "x"
version = "1.0.0"
bogus = "y"

[whatever]
hello = 1
"#;
    let r: Result<ToolToml, _> = toml::from_str(toml);
    println!("unknown-keys => {:?}", r);
    assert!(r.is_ok(), "unknown keys must be ignored");
}

#[test]
fn spec_missing_tool_section_fails() {
    let toml = r#"
[skills]
import_on_register = true
"#;
    let r: Result<ToolToml, _> = toml::from_str(toml);
    println!("missing-tool => {:?}", r);
    assert!(r.is_err());
}

#[test]
fn spec_launch_section_requires_command() {
    let toml = r#"
[tool]
name = "x"
version = "1.0.0"

[launch]
runtime = "process"
"#;
    let r: Result<ToolToml, _> = toml::from_str(toml);
    println!("launch-missing-command => {:?}", r);
    assert!(r.is_err(), "launch.command is required");
}

#[test]
fn spec_launch_defaults_and_full_toml() {
    let toml = r#"
[tool]
name = "acme"
version = "1.0.0"

[skills]
native_dir = "~/.acme/skills"
import_on_register = true

[mcp]
config_path = "~/.acme/mcp.json"
shared_servers = ["filesystem"]
import_on_register = true

[auth]
store_path = "~/.acme/auth.json"
providers = ["anthropic"]
import_on_register = true

[hooks]
scripts_dir = "~/.acme/hooks"
register_on_start = true

[routing]
default_tier = "mid"

[sessions]
store_path = "~/.acme/sessions"
auto_export = true

[launch]
runtime = "process"
command = "acme"
args = ["--acp"]
"#;
    let cfg: ToolToml = toml::from_str(toml).unwrap();
    assert_eq!(cfg.skills.as_ref().unwrap().extension, "md");
    assert_eq!(cfg.hooks.as_ref().unwrap().default_timeout_seconds, 30);
    assert_eq!(cfg.routing.as_ref().unwrap().strategy, "balanced");
    assert_eq!(cfg.sessions.as_ref().unwrap().max_shared_sessions, 50);
    let l = cfg.launch.as_ref().unwrap();
    assert_eq!(l.command, "acme");
    assert!(l.auto_remove);
    println!("full toml ok: {:?}", l);
}

#[test]
fn spec_adapter_from_file_in_tempdir() {
    let tmp = tempfile::tempdir().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(
        tools.join("acme.toml"),
        r#"
[tool]
name = "acme"
version = "1.0.0"

[skills]
native_dir = "~/.acme/skills"
import_on_register = true

[launch]
command = "acme"
args = ["--acp"]
"#,
    )
    .unwrap();
    let a = GenericAdapter::new(tmp.path().to_str().unwrap(), "acme").unwrap();
    assert!(a.has_skills());
    assert!(!a.has_mcp());
    let lc = a.launch_config().unwrap();
    assert_eq!(lc.command, "acme");
    assert_eq!(lc.args, vec!["--acp".to_string()]);
    println!("adapter ok, launch = {:?}", lc);
}

#[test]
fn spec_absent_sections_are_noops() {
    let tmp = tempfile::tempdir().unwrap();
    let tools = tmp.path().join("tools");
    std::fs::create_dir_all(&tools).unwrap();
    std::fs::write(
        tools.join("bare.toml"),
        "[tool]\nname = \"bare\"\nversion = \"1.0.0\"\n",
    )
    .unwrap();
    let a = GenericAdapter::new(tmp.path().to_str().unwrap(), "bare").unwrap();
    assert_eq!(a.absent_features().len(), 6);
    let b = a.builder();
    assert!(b.build_skill_provider().is_none());
    assert!(b.build_mcp_consumer().is_none());
    assert!(b.build_auth_provider().is_none());
    assert!(b.build_hook_executor().is_none());
    assert!(b.build_routing_target().is_none());
    assert!(b.build_session_exportable().is_none());
    println!("absent sections -> None builders ok");
}
