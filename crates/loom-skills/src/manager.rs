use anyhow::Result;

pub struct SkillManager {
    #[allow(dead_code)]
    loom_dir: String,
}

impl SkillManager {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
        }
    }

    pub fn install(&self, _name: &str, _content: &str) -> Result<()> {
        Ok(())
    }

    pub fn remove(&self, _name: &str) -> Result<()> {
        Ok(())
    }
}
