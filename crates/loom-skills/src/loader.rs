use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use tracing::debug;

/// Two-tier index for skills: lightweight metadata in memory, full content loaded on-demand
pub struct SkillLoader {
    /// Base directory where skills are stored
    loom_dir: PathBuf,
    /// Lightweight metadata cache (~50 bytes per entry in practice)
    /// In a real implementation, this would be stored in SQLite, but we'll keep it in memory for simplicity
    metadata_cache: Arc<RwLock<HashMap<String, SkillMetadata>>>,
    /// Full content cache (LRU could be added in production)
    content_cache: Arc<RwLock<HashMap<String, String>>>,
    /// Maximum number of entries to keep in content cache
    max_content_cache_size: usize,
}

#[derive(Debug, Clone)]
pub struct SkillMetadata {
    /// Skill name (used as key)
    pub name: String,
    /// File path to the skill
    pub path: PathBuf,
    /// Content hash for change detection
    pub content_hash: String,
    /// Last modified timestamp
    pub last_modified: String,
}

impl SkillLoader {
    /// Create a new skill loader
    pub fn new(loom_dir: &str) -> Self {
        let loom_dir_path = PathBuf::from(loom_dir);
        Self {
            loom_dir: loom_dir_path.clone(),
            metadata_cache: Arc::new(RwLock::new(HashMap::new())),
            content_cache: Arc::new(RwLock::new(HashMap::new())),
            max_content_cache_size: 100, // Keep 100 skill contents in memory by default
        }
    }

    /// Scan the skills directory and populate metadata cache
    pub fn refresh_metadata(&self) -> anyhow::Result<()> {
        let skills_dir = self.loom_dir.join("skills");
        if !skills_dir.exists() {
            return Ok(());
        }

        let mut metadata = HashMap::new();

        for entry in std::fs::read_dir(&skills_dir)? {
            let entry = entry?;
            let path = entry.path();

            // Only process skill files (we'll look for .md files for now)
            if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
                if let Some(name) = path.file_stem().and_then(|s| s.to_str()) {
                    // Get file metadata
                    if let Ok(metadata_file) = std::fs::metadata(&path) {
                        if let Ok(modified) = metadata_file.modified() {
                            let modified_str = modified
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs()
                                .to_string();

                            // Calculate a simple hash based on file size and mtime for change detection
                            // In production, we'd use a real hash of the content
                            let content_hash = format!(
                                "{:x}",
                                metadata_file.len().wrapping_add(
                                    modified
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap()
                                        .as_secs()
                                )
                            );

                            metadata.insert(
                                name.to_string(),
                                SkillMetadata {
                                    name: name.to_string(),
                                    path: path.clone(),
                                    content_hash,
                                    last_modified: modified_str,
                                },
                            );
                        }
                    }
                }
            }
        }

        // Update the cache
        *self.metadata_cache.write().unwrap() = metadata.clone();

        debug!(
            skill_count = metadata.len(),
            "Refreshed skill metadata cache"
        );

        Ok(())
    }

    /// Get metadata for a skill by name
    pub fn get_metadata(&self, name: &str) -> Option<SkillMetadata> {
        let cache = self.metadata_cache.read().unwrap();
        cache.get(name).cloned()
    }

    /// List all skill metadata
    pub fn list_metadata(&self) -> Vec<SkillMetadata> {
        let cache = self.metadata_cache.read().unwrap();
        cache.values().cloned().collect()
    }

    /// Load full skill content on-demand
    pub fn load_content(&self, name: &str) -> anyhow::Result<Option<String>> {
        // Check content cache first
        {
            let cache = self.content_cache.read().unwrap();
            if let Some(content) = cache.get(name) {
                return Ok(Some(content.clone()));
            }
        }

        // Get metadata to find the file path
        let metadata = match self.get_metadata(name) {
            Some(m) => m,
            None => return Ok(None),
        };

        // Read the file content
        let content = std::fs::read_to_string(&metadata.path)?;

        // Store in cache (with simple size limit)
        {
            let mut cache = self.content_cache.write().unwrap();
            if cache.len() >= self.max_content_cache_size {
                // Remove oldest entry (simple FIFO for now)
                if let Some(key) = cache.keys().next().cloned() {
                    cache.remove(&key);
                }
            }
            cache.insert(name.to_string(), content.clone());
        }

        Ok(Some(content))
    }

    /// Check if skill content has changed based on metadata
    pub fn has_content_changed(&self, name: &str, known_hash: &str) -> anyhow::Result<bool> {
        let metadata = match self.get_metadata(name) {
            Some(m) => m,
            None => return Ok(true), // Treat missing as changed
        };

        Ok(metadata.content_hash != known_hash)
    }

    /// Preload content for a list of skill names
    pub fn preload_contents(&self, names: &[String]) -> anyhow::Result<()> {
        for name in names {
            let _ = self.load_content(name)?;
        }
        Ok(())
    }

    /// Clear the content cache
    pub fn clear_content_cache(&self) {
        let mut cache = self.content_cache.write().unwrap();
        cache.clear();
    }

    /// Get current content cache size
    pub fn content_cache_size(&self) -> usize {
        let cache = self.content_cache.read().unwrap();
        cache.len()
    }

    /// Get current metadata cache size
    pub fn metadata_cache_size(&self) -> usize {
        let cache = self.metadata_cache.read().unwrap();
        cache.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_loader_creation() {
        let dir = tempdir().unwrap();
        let loader = SkillLoader::new(dir.path().to_str().unwrap());
        assert_eq!(loader.metadata_cache_size(), 0);
        assert_eq!(loader.content_cache_size(), 0);
    }

    #[test]
    fn test_refresh_metadata() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let loom_dir = dir.path().to_str().unwrap();

        // Create a test skill file
        let skills_dir = dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir)?;

        let skill_file = skills_dir.join("test-skill.md");
        std::fs::write(&skill_file, "# Test Skill\n\nThis is a test.")?;

        let loader = SkillLoader::new(loom_dir);
        loader.refresh_metadata()?;

        assert_eq!(loader.metadata_cache_size(), 1);

        let metadata = loader.get_metadata("test-skill").unwrap();
        assert_eq!(metadata.name, "test-skill");
        assert_eq!(metadata.path, skill_file);

        Ok(())
    }

    #[test]
    fn test_load_content() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let loom_dir = dir.path().to_str().unwrap();

        // Create a test skill file
        let skills_dir = dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir)?;

        let skill_content = "# Test Skill\n\nThis is a test skill for loading.";
        let skill_file = skills_dir.join("load-test.md");
        std::fs::write(&skill_file, skill_content)?;

        let loader = SkillLoader::new(loom_dir);
        loader.refresh_metadata()?;

        // Load the content
        let content = loader.load_content("load-test")?.unwrap();
        assert_eq!(content, skill_content);

        // Check that it's cached
        assert_eq!(loader.content_cache_size(), 1);

        // Load again - should come from cache
        let content2 = loader.load_content("load-test")?.unwrap();
        assert_eq!(content2, skill_content);

        Ok(())
    }

    #[test]
    fn test_nonexistent_skill() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let loader = SkillLoader::new(dir.path().to_str().unwrap());
        loader.refresh_metadata()?;

        let content = loader.load_content("nonexistent")?;
        assert!(content.is_none());

        Ok(())
    }

    #[test]
    fn test_has_content_changed() -> anyhow::Result<()> {
        let dir = tempdir()?;
        let loom_dir = dir.path().to_str().unwrap();

        // Create a test skill file
        let skills_dir = dir.path().join("skills");
        std::fs::create_dir_all(&skills_dir)?;

        let skill_file = skills_dir.join("change-test.md");
        let initial_content = "# Initial Content";
        std::fs::write(&skill_file, initial_content)?;

        let loader = SkillLoader::new(loom_dir);
        loader.refresh_metadata()?;

        let metadata = loader.get_metadata("change-test").unwrap();
        let initial_hash = &metadata.content_hash;

        // Initially should not have changed (same hash)
        assert!(!loader.has_content_changed("change-test", initial_hash)?);

        // Change the content
        let new_content = "# Changed Content\n\nMore content here.";
        std::fs::write(&skill_file, new_content)?;

        // Refresh metadata to get new hash
        loader.refresh_metadata()?;
        let metadata = loader.get_metadata("change-test").unwrap();
        let new_hash = &metadata.content_hash;

        // Now should report as changed
        assert!(loader.has_content_changed("change-test", initial_hash)?);
        assert!(!loader.has_content_changed("change-test", new_hash)?);

        Ok(())
    }
}
