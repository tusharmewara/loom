//! Directory scanner for `~/.agents/tools/` — discovers all registered TOML files.

use crate::config::ToolToml;
use anyhow::Result;
use std::path::Path;

/// Result of scanning one TOML file.
#[derive(Debug, Clone)]
pub struct ToolRegistration {
    /// Tool display name.
    pub name: String,
    /// Path to the TOML file.
    pub path: String,
    /// Parsed configuration.
    pub config: ToolToml,
    /// Validation warnings (non-fatal).
    pub warnings: Vec<String>,
}

/// Scans a `tools/` directory for `.toml` registrations.
pub struct AdapterDirScanner;

impl AdapterDirScanner {
    /// Scan a directory and return all valid registrations.
    ///
    /// Files that fail to parse are logged as warnings and skipped.
    pub fn scan(agents_dir: &str) -> Vec<ToolRegistration> {
        let tools_dir = Path::new(agents_dir).join("tools");
        if !tools_dir.is_dir() {
            tracing::warn!("Tools directory not found: {}", tools_dir.display());
            return vec![];
        }

        let mut registrations = Vec::new();

        let entries = match std::fs::read_dir(&tools_dir) {
            Ok(e) => e,
            Err(e) => {
                tracing::warn!("Failed to read tools dir {}: {}", tools_dir.display(), e);
                return vec![];
            }
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                continue;
            }

            match Self::parse_file(&path) {
                Ok(reg) => registrations.push(reg),
                Err(e) => {
                    tracing::warn!(
                        "Skipping invalid tool TOML {}: {}",
                        path.display(),
                        e
                    );
                }
            }
        }

        registrations
    }

    /// Parse a single TOML file and validate it.
    pub fn parse_file(path: &Path) -> Result<ToolRegistration> {
        let raw = std::fs::read_to_string(path)?;
        let config: ToolToml = toml::from_str(&raw)?;
        let mut warnings = Vec::new();

        // Validation warnings (non-fatal)
        if config.tool.name.trim().is_empty() {
            warnings.push("tool.name is empty".into());
        }
        if config.tool.version.trim().is_empty() {
            warnings.push("tool.version is empty".into());
        }

        if let Some(ref skills) = config.skills {
            if skills.native_dir.is_none() && !skills.import_on_register {
                warnings.push("skills section present but no native_dir and import_on_register is false".into());
            }
        }

        let name = config.tool.name.clone();
        Ok(ToolRegistration {
            name,
            path: path.to_string_lossy().into_owned(),
            config,
            warnings,
        })
    }

    /// Count TOML files in a directory without full parsing.
    pub fn count(agents_dir: &str) -> usize {
        let tools_dir = Path::new(agents_dir).join("tools");
        if !tools_dir.is_dir() {
            return 0;
        }
        std::fs::read_dir(&tools_dir)
            .map(|entries| {
                entries
                    .flatten()
                    .filter(|e| e.path().extension().and_then(|e| e.to_str()) == Some("toml"))
                    .count()
            })
            .unwrap_or(0)
    }
}
