//! The tool TOML example published in
//! `docs/protocol/adapter-and-acp-protocol.md` §1.5 must parse.
//!
//! The example is read straight out of the spec, so the document and this
//! test can never drift apart: if the published example changes, this test
//! changes with it. If the spec file cannot be found, the test fails — a
//! silently skipped check is worse than a broken one.

use std::path::{Path, PathBuf};

use loom_generic_adapter::config::ToolToml;
use loom_generic_adapter::GenericAdapter;

fn spec_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/protocol/adapter-and-acp-protocol.md")
}

/// The first fenced ```toml block after the `### 1.5` heading.
fn published_example(spec: &str) -> String {
    let section = spec
        .split("### 1.5")
        .nth(1)
        .expect("spec has a §1.5 section");
    let block = section
        .split("```toml")
        .nth(1)
        .expect("§1.5 contains a ```toml block");
    let body = block
        .split("```")
        .next()
        .expect("the fenced block is terminated");
    // The block carries a `# ~/.loom/tools/acme.toml` comment line; keep it,
    // it is part of what the spec publishes.
    assert!(body.contains("[tool]"), "example lost its [tool] section");
    body.to_string()
}

#[test]
fn published_spec_example_parses_and_loads() {
    let spec = std::fs::read_to_string(spec_path())
        .expect("read docs/protocol/adapter-and-acp-protocol.md");
    let example = published_example(&spec);

    // The published shape must satisfy the real schema.
    let parsed: ToolToml = toml::from_str(&example).expect("the published example parses");

    assert_eq!(parsed.tool.name, "acme");
    assert_eq!(parsed.tool.version, "1.0.0");
    assert!(parsed.launch.is_some(), "example has a [launch] section");
    assert_eq!(
        parsed.launch.as_ref().map(|l| l.command.as_str()),
        Some("acme")
    );

    // And it must be loadable the way Loom actually loads a tool: from
    // `{loom_dir}/tools/<name>.toml`.
    let dir = tempfile::tempdir().expect("tempdir");
    let tools = dir.path().join("tools");
    std::fs::create_dir_all(&tools).expect("create tools dir");
    std::fs::write(tools.join("acme.toml"), &example).expect("write tool file");

    let adapter = GenericAdapter::new(dir.path().to_str().unwrap(), "acme")
        .expect("GenericAdapter loads the published example");
    assert_eq!(adapter.tool_name(), "acme");
    assert!(
        adapter.launch_config().is_some(),
        "launch config is derived"
    );
}
