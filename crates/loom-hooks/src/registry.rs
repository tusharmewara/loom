use anyhow::Result;
use loom_core::HookEntry;
use std::collections::HashMap;
use std::path::Path;

pub struct HookRegistry {
    agents_dir: String,
    hooks: HashMap<String, HookEntry>,
}

impl HookRegistry {
    pub fn new(agents_dir: &str) -> Self {
        let mut registry = Self {
            agents_dir: agents_dir.to_string(),
            hooks: HashMap::new(),
        };
        registry.reload();
        registry
    }

    pub fn reload(&mut self) {
        self.hooks.clear();
        let hooks_dir = Path::new(&self.agents_dir).join("hooks");
        if !hooks_dir.exists() {
            return;
        }
        let entries = match std::fs::read_dir(&hooks_dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                    if let Ok(raw) = std::fs::read_to_string(&path) {
                        if let Ok(hook) = toml::from_str::<HookEntry>(&raw) {
                            self.hooks.insert(hook.id.clone(), hook);
                        }
                    }
                }
            }
        }
    }

    pub fn list(&self) -> Vec<HookEntry> {
        let mut v: Vec<_> = self.hooks.values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        v
    }

    pub fn get(&self, id: &str) -> Option<&HookEntry> {
        self.hooks.get(id)
    }

    pub fn register(&mut self, entry: &HookEntry) -> Result<()> {
        let hooks_dir = Path::new(&self.agents_dir).join("hooks");
        std::fs::create_dir_all(&hooks_dir)?;
        let path = hooks_dir.join(format!("{}.toml", entry.id));
        let raw = toml::to_string_pretty(entry)?;
        std::fs::write(&path, raw)?;
        self.hooks.insert(entry.id.clone(), entry.clone());
        Ok(())
    }

    pub fn remove(&mut self, id: &str) -> Result<bool> {
        let path = Path::new(&self.agents_dir)
            .join("hooks")
            .join(format!("{id}.toml"));
        let removed = if path.exists() {
            std::fs::remove_file(&path)?;
            true
        } else {
            false
        };
        self.hooks.remove(id);
        Ok(removed)
    }
}
