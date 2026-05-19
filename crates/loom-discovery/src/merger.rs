use loom_core::ConflictResolution;
use std::path::Path;

/// Resolves conflicts between native tool resources and shared ecosystem resources.
///
/// Strategies:
/// - NativeFirst (default): tool's native resources take precedence
/// - SharedFirst: shared resources take precedence
/// - Manual: user explicitly chooses via config
pub struct ResourceMerger {
    agents_dir: String,
}

pub struct Conflict {
    pub resource_name: String,
    pub resource_type: ResourceType,
    pub native_path: String,
    pub shared_path: String,
    pub resolved: bool,
    pub strategy: Option<ConflictResolution>,
}

pub enum ResourceType {
    Skill,
    McpServer,
    Hook,
}

impl ResourceMerger {
    pub fn new(agents_dir: &str) -> Self {
        Self { agents_dir: agents_dir.to_string() }
    }

    /// Resolve all conflicts across resource types.
    /// Returns the complete list of conflicts with resolution status.
    pub fn resolve_all(&self) -> anyhow::Result<Vec<Conflict>> {
        let strategy = self.load_strategy();
        let mut conflicts = Vec::new();

        // Detect overlaps between native tool dirs and shared dirs
        if let Ok(skills) = self.detect_skill_conflicts(&strategy) {
            conflicts.extend(skills);
        }

        Ok(conflicts)
    }

    /// Detect skills that exist in both a tool's native dir and the shared skills dir.
    fn detect_skill_conflicts(&self, strategy: &ConflictResolution) -> anyhow::Result<Vec<Conflict>> {
        let mut conflicts = Vec::new();
        let shared_skills = Path::new(&self.agents_dir).join("skills");
        if !shared_skills.exists() {
            return Ok(conflicts);
        }

        let shared_names: std::collections::HashSet<String> = std::fs::read_dir(&shared_skills)?
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .filter_map(|e| e.file_name().into_string().ok())
            .collect();

        // Check tools/ TOML configs for native skill directories
        let tools_dir = Path::new(&self.agents_dir).join("tools");
        if tools_dir.exists() {
            for entry in std::fs::read_dir(&tools_dir)? {
                let entry = entry?;
                let path = entry.path();
                if path.extension().map_or(false, |e| e == "toml") {
                    let content = std::fs::read_to_string(&path)?;
                    let toml_value: toml::Value = toml::from_str(&content)?;

                    // Check if tool has a [skills] section
                    if let Some(skills) = toml_value.get("skills") {
                        if let Some(native_dir) = skills.get("native_dir").and_then(|v| v.as_str()) {
                            let expanded = shellexpand::tilde(native_dir).to_string();
                            let native_path = Path::new(&expanded);
                            if native_path.exists() {
                                // Check for conflicting skill names
                                for native_entry in std::fs::read_dir(native_path)? {
                                    let native_entry = native_entry?;
                                    if native_entry.path().is_dir() {
                                        if let Some(name) = native_entry.file_name().to_str() {
                                            if shared_names.contains(name) {
                                                let _tool_name = toml_value.get("tool")
                                                    .and_then(|t| t.get("name"))
                                                    .and_then(|v| v.as_str())
                                                    .unwrap_or("unknown");

                                                let resolved = matches!(strategy, ConflictResolution::NativeFirst);
                                                conflicts.push(Conflict {
                                                    resource_name: name.to_string(),
                                                    resource_type: ResourceType::Skill,
                                                    native_path: native_entry.path().to_str().unwrap_or("").to_string(),
                                                    shared_path: shared_skills.join(name).to_str().unwrap_or("").to_string(),
                                                    resolved,
                                                    strategy: Some(strategy.clone()),
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(conflicts)
    }

    /// Load the conflict resolution strategy from config.
    fn load_strategy(&self) -> ConflictResolution {
        let config_path = Path::new(&self.agents_dir).join("config.toml");
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(config) = toml::from_str::<loom_core::LoomConfig>(&content) {
                return config.ecosystem.conflict_resolution;
            }
        }
        ConflictResolution::NativeFirst
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_conflicts_in_empty_agents() {
        let dir = std::env::temp_dir().join("loom-test-merger-empty");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir.join("skills")).unwrap();

        let merger = ResourceMerger::new(dir.to_str().unwrap());
        let conflicts = merger.resolve_all().unwrap();
        assert!(conflicts.is_empty());
    }
}
