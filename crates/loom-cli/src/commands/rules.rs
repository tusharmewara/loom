use crate::cli::RulesAction;
use loom_rules::{
    Action as RuleAction, ComparisonOp, Condition, LoomEventExt, RuleBuilder, RuleEngine,
};
use std::path::Path;

pub async fn run(loom_dir: &Path, action: &RulesAction) -> anyhow::Result<()> {
    let engine = RuleEngine::new(loom_dir.to_str().unwrap());
    engine.load_rules().await?;

    match action {
        RulesAction::List { json } => {
            let rules = engine.list_rules().await;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "command": "rules.list",
                        "count": rules.len(),
                        "rules": rules,
                    }))?
                );
            } else {
                for rule in &rules {
                    let status = if rule.enabled { "enabled" } else { "disabled" };
                    let conds = rule.conditions.len();
                    let acts = rule.actions.len();
                    println!(
                        "{}  {}  [{}]  {} conds, {} acts  prio={}",
                        rule.id, rule.name, status, conds, acts, rule.priority
                    );
                }
                if rules.is_empty() {
                    println!("No rules defined.");
                }
            }
        }
        RulesAction::Show { id, json } => {
            let rule = engine
                .get_rule(id)
                .await
                .ok_or_else(|| anyhow::anyhow!("Rule '{id}' not found"))?;
            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "command": "rules.show",
                        "rule": rule,
                    }))?
                );
            } else {
                println!("{}", toml::to_string_pretty(&rule)?);
            }
        }
        RulesAction::Add {
            id,
            name,
            event_type,
            field,
            operator,
            value,
            action_log,
            action_suppress,
            priority,
        } => {
            let mut builder = RuleBuilder::new(id, name);
            if let Some(et) = event_type {
                builder = builder.condition(Condition::event_type(et));
            }
            if let Some(f) = field {
                let op = match operator.as_deref().unwrap_or("equals") {
                    "equals" | "eq" => ComparisonOp::Equals,
                    "not-equals" | "neq" => ComparisonOp::NotEquals,
                    "gt" | "greater-than" => ComparisonOp::GreaterThan,
                    "lt" | "less-than" => ComparisonOp::LessThan,
                    "contains" => ComparisonOp::Contains,
                    "starts-with" => ComparisonOp::StartsWith,
                    "ends-with" => ComparisonOp::EndsWith,
                    other => anyhow::bail!("Unknown operator: {other}"),
                };
                let v = value.as_deref().unwrap_or("");
                builder = builder.condition(Condition::field(f, op, v));
            }
            if let Some(msg) = action_log {
                builder = builder.action(RuleAction::log(msg));
            }
            if *action_suppress {
                builder = builder.action(RuleAction::suppress());
            }
            if let Some(p) = priority {
                builder = builder.priority(*p);
            }

            let rule = builder.build();
            engine.add_rule(rule.clone()).await;
            engine.save_rule(&rule)?;
            println!("Rule '{}' added.", id);
        }
        RulesAction::Remove { id } => {
            engine.remove_rule(id).await?;
            engine.delete_rule_file(id)?;
            println!("Rule '{}' removed.", id);
        }
        RulesAction::Eval {
            event_type,
            field,
            value,
            json,
        } => {
            // Build a synthetic event for evaluation
            let ev = build_synthetic_event(event_type, field.as_deref(), value.as_deref());
            let results = engine.evaluate(&ev).await?;

            if *json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({
                        "command": "rules.eval",
                        "event": ev.variant_name(),
                        "results": results,
                    }))?
                );
            } else {
                println!(
                    "Event: {} (total evaluations: {})",
                    ev.variant_name(),
                    engine.total_evaluations()
                );
                for r in &results {
                    let status = if r.matched { "✓" } else { " " };
                    println!(
                        "  {} {}  matched={}  actions={:?}",
                        status, r.rule_id, r.matched, r.actions_fired
                    );
                }
            }
        }
    }
    Ok(())
}

/// Build a synthetic event for `loom rules eval` testing.
fn build_synthetic_event(
    event_type: &str,
    field: Option<&str>,
    value: Option<&str>,
) -> loom_rules::LoomEvent {
    // Build a reasonable synthetic event based on the type
    match event_type {
        "SkillsAdded" => {
            let count = value.and_then(|v| v.parse().ok()).unwrap_or(1);
            loom_rules::LoomEvent::SkillsAdded { count }
        }
        "SkillsRemoved" => {
            let name = value.unwrap_or("test-skill").to_string();
            loom_rules::LoomEvent::SkillsRemoved { name }
        }
        "McpHealthChanged" => {
            let id = field.unwrap_or("server-1").to_string();
            loom_rules::LoomEvent::McpHealthChanged {
                id,
                status: loom_core::HealthStatusStr::Unknown,
            }
        }
        "McpServerAdded" => {
            let name = value.unwrap_or("server-name").to_string();
            loom_rules::LoomEvent::McpServerAdded {
                id: field.unwrap_or("srv-1").to_string(),
                name,
            }
        }
        "SessionExported" => loom_rules::LoomEvent::SessionExported {
            session_id: value.unwrap_or("session-1").to_string(),
        },
        "RoutingUpdated" => loom_rules::LoomEvent::RoutingUpdated,
        "AuthProviderAdded" => loom_rules::LoomEvent::AuthProviderAdded {
            provider_id: value.unwrap_or("provider-1").to_string(),
        },
        "HooksRegistered" => loom_rules::LoomEvent::HooksRegistered {
            hook_id: value.unwrap_or("hook-1").to_string(),
        },
        "TaskPhaseChanged" => loom_rules::LoomEvent::TaskPhaseChanged {
            task_id: field.unwrap_or("task-1").to_string(),
            phase: loom_core::TaskPhase::Implement,
        },
        "TaskCreated" => loom_rules::LoomEvent::TaskCreated {
            task_id: value.unwrap_or("task-1").to_string(),
        },
        _ => {
            // Default to RoutingUpdated as a safe fallback
            loom_rules::LoomEvent::RoutingUpdated
        }
    }
}
