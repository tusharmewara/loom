//! Workflow template manager.
//!
//! Templates are TOML files stored in `~/.loom/workflow/templates/<name>.toml`.
//! Each template pre-configures fields for task creation: title, description,
//! agent, sub-agents, provider, and model.

use loom_core::WorkflowTemplate;
use std::path::{Path, PathBuf};

/// Manages loading, listing, and saving workflow templates.
pub struct TemplateManager {
    templates_dir: PathBuf,
}

impl TemplateManager {
    /// Create a new manager, ensuring the templates directory exists.
    pub fn new(loom_dir: &str) -> Self {
        let templates_dir = PathBuf::from(loom_dir).join("workflow").join("templates");
        // Ensure directory exists (best-effort)
        let _ = std::fs::create_dir_all(&templates_dir);
        Self { templates_dir }
    }

    /// List all available templates.
    pub fn list(&self) -> Vec<WorkflowTemplate> {
        let mut templates = Vec::new();
        let dir = match std::fs::read_dir(&self.templates_dir) {
            Ok(d) => d,
            Err(_) => return templates,
        };
        for entry in dir.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                if let Ok(t) = self.load_from_path(&path) {
                    templates.push(t);
                }
            }
        }
        templates.sort_by(|a, b| a.name.cmp(&b.name));
        templates
    }

    /// Load a single template by name.
    pub fn get(&self, name: &str) -> Option<WorkflowTemplate> {
        let path = self.templates_dir.join(format!("{name}.toml"));
        if !path.exists() {
            return None;
        }
        self.load_from_path(&path).ok()
    }

    /// Save a template (create or overwrite).
    pub fn save(&self, template: &WorkflowTemplate) -> std::io::Result<()> {
        let path = self.templates_dir.join(format!("{}.toml", template.name));
        let toml_str = toml::to_string_pretty(template)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        std::fs::write(path, toml_str)
    }

    /// Delete a template by name.
    pub fn delete(&self, name: &str) -> std::io::Result<bool> {
        let path = self.templates_dir.join(format!("{name}.toml"));
        if path.exists() {
            std::fs::remove_file(path)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn load_from_path(&self, path: &Path) -> Result<WorkflowTemplate, anyhow::Error> {
        let raw = std::fs::read_to_string(path)?;
        let template: WorkflowTemplate = toml::from_str(&raw)?;
        Ok(template)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_list_empty_dir() {
        let dir = tempdir().unwrap();
        let mgr = TemplateManager::new(dir.path().to_str().unwrap());
        let templates = mgr.list();
        assert!(templates.is_empty());
    }

    #[test]
    fn test_save_and_load() {
        let dir = tempdir().unwrap();
        let mgr = TemplateManager::new(dir.path().to_str().unwrap());

        let template = WorkflowTemplate {
            name: "bug-fix".into(),
            description: "Quick bug fix workflow".into(),
            default_title: Some("Fix: {issue_title}".into()),
            default_description: None,
            default_agent: Some("jcode".into()),
            sub_agents: None,
            default_provider: None,
            default_model: None,
        };

        mgr.save(&template).unwrap();
        let loaded = mgr.get("bug-fix").unwrap();
        assert_eq!(loaded.name, "bug-fix");
        assert_eq!(loaded.default_agent.as_deref(), Some("jcode"));

        let all = mgr.list();
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn test_get_missing_returns_none() {
        let dir = tempdir().unwrap();
        let mgr = TemplateManager::new(dir.path().to_str().unwrap());
        assert!(mgr.get("nonexistent").is_none());
    }

    #[test]
    fn test_delete() {
        let dir = tempdir().unwrap();
        let mgr = TemplateManager::new(dir.path().to_str().unwrap());

        let template = WorkflowTemplate {
            name: "temp".into(),
            description: "temp".into(),
            default_title: None,
            default_description: None,
            default_agent: None,
            sub_agents: None,
            default_provider: None,
            default_model: None,
        };
        mgr.save(&template).unwrap();
        assert!(mgr.get("temp").is_some());

        mgr.delete("temp").unwrap();
        assert!(mgr.get("temp").is_none());
    }
}
