use crate::cli::SkillsAction;
use loom_ipc::IpcRequest;
use loom_skills::lockfile::{self, SkillsLock};
use std::path::Path;

fn update_lock(loom_dir: &Path) -> anyhow::Result<()> {
    let engine = loom_skills::SkillsEngine::new(loom_dir.to_str().unwrap())?;
    let registry_skills = engine.registry().list();
    let lock_path = lockfile::default_lock_path(loom_dir.to_str().unwrap());
    let mut lock = SkillsLock::load(&lock_path)?.unwrap_or_default();
    for skill in &registry_skills {
        lock.lock_skill(skill);
    }
    lock.save(&lock_path)?;
    Ok(())
}

pub async fn run(loom_dir: &Path, action: &SkillsAction) -> anyhow::Result<()> {
    match action {
        SkillsAction::List { json, locked } => {
            let req = IpcRequest::new("skills.list");
            match crate::ipc_client::send_daemon_command(loom_dir, req).await {
                Ok(resp) if resp.status == "ok" => {
                    if let Some(data) = resp.data {
                        if let Some(skills) = data.get("skills") {
                            if *json {
                                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                    "schema_version": 1,
                                    "command": "skills.list",
                                    "count": skills.as_array().map(|a| a.len()).unwrap_or(0),
                                    "skills": skills
                                }))?);
                            } else {
                                println!("{}", serde_json::to_string_pretty(skills)?);
                            }
                            return Ok(());
                        }
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("Daemon unavailable ({e}); listing local skills dir"),
            }

            let skills_dir = loom_dir.join("skills");
            let mut skill_names = Vec::new();
            if skills_dir.exists() {
                for entry in std::fs::read_dir(&skills_dir)? {
                    let name = entry?.file_name().to_string_lossy().to_string();
                    skill_names.push(name.clone());
                }
            }
            if *json {
                println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                    "schema_version": 1,
                    "command": "skills.list",
                    "count": skill_names.len(),
                    "skills": skill_names
                }))?);
            } else {
                if skill_names.is_empty() {
                    println!("No skills found.");
                } else {
                    for name in &skill_names {
                        println!("{}", name);
                    }
                }
            }
            if *locked {
                let lock_path = lockfile::default_lock_path(loom_dir.to_str().unwrap());
                match SkillsLock::load(&lock_path) {
                    Ok(Some(lock)) => {
                        let loom_str = loom_dir.to_str().unwrap();
                        if let Ok(engine) = loom_skills::SkillsEngine::new(loom_str) {
                            let skills = engine.registry().list();
                            if lock.is_clean(&skills) {
                                println!("  ✓ Lock file clean");
                            } else {
                                let violations = lock.verify_against(&skills);
                                for v in &violations {
                                    match v.kind {
                                        lockfile::LockViolationKind::Missing => {
                                            println!("  ✗ {}: in lock but removed from registry", v.name);
                                        }
                                        lockfile::LockViolationKind::VersionMismatch => {
                                            println!("  ✗ {}: version mismatch (expected {}, actual {})", v.name, v.expected, v.actual);
                                        }
                                        lockfile::LockViolationKind::ContentChanged => {
                                            println!("  ✗ {}: content hash changed", v.name);
                                        }
                                        lockfile::LockViolationKind::Unlocked => {
                                            println!("  ✗ {}: not in lock file", v.name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Ok(None) => println!("  ℹ No lock file found (run `loom lock --generate`)"),
                    Err(e) => eprintln!("  ⚠ Failed to read lock file: {e}"),
                }
            }
        }
        SkillsAction::Show { name, json } => {
            let req = IpcRequest::new("skills.show")
                .with_params(serde_json::json!({ "name": name }));
            match crate::ipc_client::send_daemon_command(loom_dir, req).await {
                Ok(resp) if resp.status == "ok" => {
                    if let Some(data) = resp.data {
                        if *json {
                            println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                                "schema_version": 1,
                                "command": "skills.show",
                                "name": name,
                                "skill": data
                            }))?);
                        } else {
                            println!("{}", serde_json::to_string_pretty(&data)?);
                        }
                        return Ok(());
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("Daemon unavailable ({e})"),
            }

            let path = loom_dir.join("skills").join(name);
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                if *json {
                    println!("{}", serde_json::to_string_pretty(&serde_json::json!({
                        "schema_version": 1,
                        "command": "skills.show",
                        "name": name,
                        "content": content
                    }))?);
                } else {
                    println!("{}", content);
                }
            } else {
                anyhow::bail!("Skill '{name}' not found");
            }
        }
        SkillsAction::Install { path } => {
            let src = Path::new(path);
            if !src.exists() {
                anyhow::bail!("Path does not exist: {path}");
            }
            let name = src
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            let dst = loom_dir.join("skills").join(src.file_name().unwrap());
            std::fs::create_dir_all(loom_dir.join("skills"))?;
            std::fs::copy(src, &dst)?;
            println!("Installed skill '{name}'");
            update_lock(loom_dir)?;
        }
        SkillsAction::Remove { name } => {
            let path = loom_dir.join("skills").join(name);
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("Removed skill '{name}'");
                update_lock(loom_dir)?;
            } else {
                anyhow::bail!("Skill '{name}' not found");
            }
        }
    }
    Ok(())
}
