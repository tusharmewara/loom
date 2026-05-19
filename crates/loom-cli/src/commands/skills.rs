use crate::cli::SkillsAction;
use loom_ipc::IpcRequest;
use std::path::Path;

pub async fn run(agents_dir: &Path, action: &SkillsAction) -> anyhow::Result<()> {
    match action {
        SkillsAction::List { json } => {
            let req = IpcRequest::new("skills.list");
            match crate::ipc_client::send_daemon_command(agents_dir, req).await {
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

            let skills_dir = agents_dir.join("skills");
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
        }
        SkillsAction::Show { name, json } => {
            let req = IpcRequest::new("skills.show")
                .with_params(serde_json::json!({ "name": name }));
            match crate::ipc_client::send_daemon_command(agents_dir, req).await {
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

            let path = agents_dir.join("skills").join(name);
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
            let dst = agents_dir.join("skills").join(src.file_name().unwrap());
            std::fs::create_dir_all(agents_dir.join("skills"))?;
            std::fs::copy(src, &dst)?;
            println!("Installed skill '{name}'");
        }
        SkillsAction::Remove { name } => {
            let path = agents_dir.join("skills").join(name);
            if path.exists() {
                std::fs::remove_file(&path)?;
                println!("Removed skill '{name}'");
            } else {
                anyhow::bail!("Skill '{name}' not found");
            }
        }
    }
    Ok(())
}
