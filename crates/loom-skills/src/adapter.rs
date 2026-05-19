use std::collections::HashMap;

/// Per-tool string replacements when importing a neutral skill.
#[derive(Debug, Clone, Default)]
pub struct ToolAdapterConfig {
    pub replacements: HashMap<String, String>,
}

impl ToolAdapterConfig {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_replacement(mut self, from: impl Into<String>, to: impl Into<String>) -> Self {
        self.replacements.insert(from.into(), to.into());
        self
    }
}

/// Map platform-specific references in skill body to the importing tool's conventions.
pub fn adapt_skill(content: &str, config: &ToolAdapterConfig) -> String {
    let mut out = content.to_string();
    let mut keys: Vec<_> = config.replacements.keys().collect();
    keys.sort_by_key(|k| std::cmp::Reverse(k.len()));
    for from in keys {
        if let Some(to) = config.replacements.get(from) {
            out = out.replace(from, to);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_adapt_skill_replacements() {
        let cfg = ToolAdapterConfig::new()
            .with_replacement("~/.claude/", "~/.cursor/")
            .with_replacement("Claude", "Cursor");
        let out = adapt_skill("Use Claude with ~/.claude/skills", &cfg);
        assert!(out.contains("Cursor"));
        assert!(out.contains("~/.cursor/"));
    }
}
