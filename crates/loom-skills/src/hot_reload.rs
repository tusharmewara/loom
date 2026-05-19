use anyhow::Result;
use loom_core::LoomEvent;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::sync::mpsc::channel;
use std::thread;
use std::time::Duration;

/// Watch `~/.loom/skills/` and emit events on SKILL.md changes.
pub struct SkillWatcher {
    loom_dir: String,
    handle: Option<thread::JoinHandle<()>>,
}

impl SkillWatcher {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
            handle: None,
        }
    }

    pub fn start<F>(&mut self, callback: F) -> Result<()>
    where
        F: Fn(LoomEvent) + Send + 'static,
    {
        let skills_path = Path::new(&self.loom_dir).join("skills");
        if !skills_path.exists() {
            std::fs::create_dir_all(&skills_path)?;
        }

        let (tx, rx) = channel();
        let mut watcher = RecommendedWatcher::new(
            move |res| {
                if let Ok(event) = res {
                    let _ = tx.send(event);
                }
            },
            Config::default().with_poll_interval(Duration::from_millis(100)),
        )?;

        watcher.watch(&skills_path, RecursiveMode::Recursive)?;

        let handle = thread::spawn(move || {
            let _watcher = watcher;
            for event in rx {
                let kind = match event.kind {
                    notify::EventKind::Create(_) => "added",
                    notify::EventKind::Modify(_) => "modified",
                    notify::EventKind::Remove(_) => "removed",
                    _ => continue,
                };
                let skill_name = event
                    .paths
                    .iter()
                    .find_map(|p| {
                        p.file_name()
                            .and_then(|n| n.to_str())
                            .filter(|n| *n == "SKILL.md" || n.ends_with(".md"))
                            .and_then(|_| {
                                p.parent()
                                    .and_then(|parent| parent.file_name())
                                    .and_then(|n| n.to_str())
                                    .map(|s| s.to_string())
                            })
                    })
                    .unwrap_or_else(|| "unknown".into());

                match kind {
                    "added" => callback(LoomEvent::SkillsAdded { count: 1 }),
                    "removed" => callback(LoomEvent::SkillsRemoved { name: skill_name }),
                    _ => callback(LoomEvent::SkillsAdded { count: 1 }),
                }
            }
        });

        self.handle = Some(handle);
        Ok(())
    }

    pub fn stop(&mut self) {
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for SkillWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}
