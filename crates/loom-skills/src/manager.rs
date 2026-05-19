use anyhow::Result;

pub struct SkillManager {
    #[allow(dead_code)]
    agents_dir: String,
}

impl SkillManager {
    pub fn new(agents_dir: &str) -> Self {
        Self {
            agents_dir: agents_dir.to_string(),
        }
    }

    pub fn install(&self, _name: &str, _content: &str) -> Result<()> {
        Ok(())
    }

    pub fn remove(&self, _name: &str) -> Result<()> {
        Ok(())
    }
}
