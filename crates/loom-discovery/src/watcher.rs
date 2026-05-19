use loom_core::LoomEvent;
use std::path::Path;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;
use std::time::Duration;
use std::thread;

/// Filesystem watcher that propagates changes to the EventBus.
///
/// Uses notify crate (inotify on Linux, FSEvents on macOS).
/// Detects file changes within 100ms and forwards typed events.
pub struct DiscoveryWatcher {
    loom_dir: String,
    watcher_handle: Option<thread::JoinHandle<()>>,
}

impl DiscoveryWatcher {
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
            watcher_handle: None,
        }
    }

    /// Start watching the agents directory for changes.
    /// Calls `callback` with each detected event.
    pub fn start_watching<F>(&mut self, callback: F) -> anyhow::Result<()>
    where
        F: Fn(LoomEvent) + Send + 'static,
    {
        let agents_path = Path::new(&self.loom_dir);
        if !agents_path.exists() {
            tracing::warn!("Cannot watch {}: directory does not exist", self.loom_dir);
            return Ok(());
        }

        let (tx, rx) = channel();
        let loom_dir = self.loom_dir.clone();

        let mut watcher = RecommendedWatcher::new(
            move |res: notify::Result<notify::Event>| {
                if let Ok(event) = res {
                    let loom_event = Self::map_notify_event(&event, &loom_dir);
                    if let Some(ev) = loom_event {
                        let _ = tx.send(ev);
                    }
                }
            },
            Config::default()
                .with_poll_interval(Duration::from_secs(2))
        )?;

        watcher.watch(agents_path, RecursiveMode::Recursive)?;

        let handle = thread::spawn(move || {
            for event in rx {
                callback(event);
            }
        });

        self.watcher_handle = Some(handle);
        tracing::info!("Watching {} for filesystem changes", self.loom_dir);
        Ok(())
    }

    /// Map a notify event to a LoomEvent.
    pub fn map_notify_event(event: &notify::Event, loom_dir: &str) -> Option<LoomEvent> {
        let path = event.paths.first()?;
        let relative = path.strip_prefix(loom_dir).ok()?;
        let components: Vec<&str> = relative.components()
            .filter_map(|c| c.as_os_str().to_str())
            .collect();

        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                match *components.first()? {
                    "skills" => {
                        let _ = components.get(1);
                        Some(LoomEvent::SkillsAdded { count: 1 })
                    }
                    "mcp" => {
                        let name = components.get(1)?;
                        Some(LoomEvent::McpServerAdded {
                            id: (*name).to_string(),
                            name: (*name).to_string(),
                        })
                    }
                    "config" => {
                        Some(LoomEvent::RoutingUpdated)
                    }
                    "auth" => {
                        let name = components.get(1)?;
                        Some(LoomEvent::AuthProviderAdded {
                            provider_id: (*name).to_string(),
                        })
                    }
                    "hooks" => {
                        let name = components.get(1)?;
                        Some(LoomEvent::HooksRegistered {
                            hook_id: (*name).to_string(),
                        })
                    }
                    "tools" => {
                        let _ = components.get(1);
                        tracing::info!("Tool registration changed in tools/");
                        None
                    }
                    _ => None,
                }
            }
            EventKind::Remove(_) => {
                match *components.first()? {
                    "skills" => {
                        let name = components.get(1)?;
                        Some(LoomEvent::SkillsRemoved {
                            name: (*name).to_string(),
                        })
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

impl Drop for DiscoveryWatcher {
    fn drop(&mut self) {
        if let Some(handle) = self.watcher_handle.take() {
            handle.thread().unpark();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_map_notify_event_skill_added() {
        let event = notify::Event {
            kind: EventKind::Create(notify::event::CreateKind::File),
            paths: vec![Path::new("/tmp/.loom/skills/my-skill/SKILL.md").to_path_buf()],
            ..notify::Event::default()
        };
        let result = DiscoveryWatcher::map_notify_event(&event, "/tmp/.loom");
        assert!(result.is_some());
        match result.unwrap() {
            LoomEvent::SkillsAdded { count } => assert_eq!(count, 1),
            _ => panic!("Expected SkillsAdded"),
        }
    }

    #[test]
    fn test_map_notify_event_config_change() {
        let event = notify::Event {
            kind: EventKind::Modify(notify::event::ModifyKind::Data(notify::event::DataChange::Any)),
            paths: vec![Path::new("/home/user/.loom/config/routing.toml").to_path_buf()],
            ..notify::Event::default()
        };
        let result = DiscoveryWatcher::map_notify_event(&event, "/home/user/.loom");
        assert!(result.is_some());
        match result.unwrap() {
            LoomEvent::RoutingUpdated => {},
            _ => panic!("Expected RoutingUpdated"),
        }
    }
}
