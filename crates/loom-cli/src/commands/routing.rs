use crate::cli::RoutingAction;
use loom_core::config::ConfigReader;
use loom_core::types::{ModelTier, RoutingDecision, Task};
use loom_routing::{CatalogManager, Classifier, DecisionEngine};
use serde_json::json;
use std::fs;
use std::path::Path;

/// Query knowledge graph for entities mentioned in the prompt.
/// Returns (entity_names, numeric_boost) where boost is 0.0–0.15.
fn knowledge_context(loom_dir: &Path, prompt: &str) -> (Vec<String>, f64) {
    match loom_knowledge::KnowledgeGraph::new(loom_dir.to_str().unwrap()) {
        Ok(kg) => {
            let lower = prompt.to_lowercase();
            let mut entities = Vec::new();
            let mut boost = 0.0f64;

            // Search for entities by extracting known names from the knowledge graph
            // We look for capitalized words that might match entities
            // Simple approach: check if lowercase prompt contains entity names
            if let Ok(stats) = kg.stats() {
                if stats.entity_count == 0 {
                    return (Vec::new(), 0.0);
                }
            }

            // Get top connected entities and check if prompt mentions them
            if let Ok(stats) = kg.stats() {
                for (name, etype, degree) in &stats.top_entities {
                    if lower.contains(&name.to_lowercase()) {
                        entities.push(name.clone());
                        // Higher-degree entities → larger boost
                        let degree_boost = (*degree as f64 * 0.02).clamp(0.0, 0.08);
                        boost += degree_boost;

                        // Type-specific boost
                        let type_boost = match etype.as_str() {
                            "architecture" | "pattern" | "framework" => 0.07,
                            "tool" | "language" | "database" => 0.03,
                            "concept" => 0.02,
                            _ => 0.0,
                        };
                        boost += type_boost;
                    }
                }
            }

            // Search for the prompt terms directly in entity FTS
            if let Ok(results) = kg.search_entities(prompt, 5) {
                for entity in &results {
                    let name_lower = entity.name.to_lowercase();
                    if !entities.iter().any(|e| e.to_lowercase() == name_lower) {
                        entities.push(entity.name.clone());
                        boost += 0.03;
                    }
                }
            }

            (entities, boost.clamp(0.0, 0.25))
        }
        Err(_) => (Vec::new(), 0.0),
    }
}

/// Load a task JSON from the workflow tasks directory.
fn load_task(loom_dir: &Path, task_id: &str) -> anyhow::Result<Task> {
    let tasks_dir = loom_dir.join("workflow").join("tasks");
    let path = tasks_dir.join(format!("{task_id}.json"));
    if !path.exists() {
        anyhow::bail!("Task file not found: {}", path.display());
    }
    let raw = fs::read_to_string(&path)?;
    let task: Task = serde_json::from_str(&raw)?;
    Ok(task)
}

/// Save a task JSON.
fn save_task(loom_dir: &Path, task: &Task) -> anyhow::Result<()> {
    let tasks_dir = loom_dir.join("workflow").join("tasks");
    let path = tasks_dir.join(format!("{}.json", task.task_id));
    let content = serde_json::to_string_pretty(task)?;
    fs::write(path, content)?;
    Ok(())
}

/// Parse tier string (slm, mid, flagship) to ModelTier.
fn parse_tier(s: &str) -> anyhow::Result<ModelTier> {
    match s.to_lowercase().as_str() {
        "slm" => Ok(ModelTier::Slm),
        "mid" => Ok(ModelTier::Mid),
        "flagship" => Ok(ModelTier::Flagship),
        _ => anyhow::bail!("Invalid tier: {}. Use slm, mid, or flagship", s),
    }
}

pub async fn run(loom_dir: &Path, action: &RoutingAction) -> anyhow::Result<()> {
    // Load config once
    let config = ConfigReader::new(loom_dir.to_str().unwrap())
        .load()
        .ok()
        .and_then(|c| c.routing);

    let classifier = Classifier::new();
    let engine = DecisionEngine::with_loom_dir(loom_dir.to_str().unwrap(), config.clone())
        .unwrap_or_else(|_| DecisionEngine::new());

    match action {
        RoutingAction::Show { json } => {
            let config_path = loom_dir.join("config").join("routing.toml");
            let config_routing = if config_path.exists() {
                let content = fs::read_to_string(&config_path)?;
                if *json {
                    let parsed: toml::Value = toml::from_str(&content)?;
                    parsed
                } else {
                    println!("{}", content);
                    toml::Value::Table(Default::default())
                }
            } else {
                serde_json::from_value(serde_json::to_value(&config)?)
                    .unwrap_or(toml::Value::Table(Default::default()))
            };

            let catalog = CatalogManager::load(loom_dir.to_str().unwrap())?;
            if *json {
                let models: Vec<_> = catalog
                    .catalog()
                    .models
                    .iter()
                    .map(|m| {
                        json!({
                            "model_id": m.model_id,
                            "provider_id": m.provider_id,
                            "tier": format!("{:?}", m.tier),
                            "input_cost_per_1k": m.input_cost_per_1k,
                            "output_cost_per_1k": m.output_cost_per_1k,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "schema_version": 1,
                        "command": "routing.show",
                        "config": config_routing,
                        "model_catalog": models
                    }))?
                );
            } else {
                println!(
                    "\nModel catalog ({} entries):",
                    catalog.catalog().models.len()
                );
                for m in &catalog.catalog().models {
                    println!(
                        "  {} @ {} tier={:?} ${:.4}/${:.4} per 1k",
                        m.model_id,
                        m.provider_id,
                        m.tier,
                        m.input_cost_per_1k,
                        m.output_cost_per_1k
                    );
                }
            }
        }
        RoutingAction::Evaluate { prompt, json } => {
            let mut score = classifier.classify(prompt);
            // Inject knowledge graph context
            let (knowledge_entities, knowledge_boost) = knowledge_context(loom_dir, prompt);
            if knowledge_boost > 0.0 {
                score.numeric = (score.numeric + knowledge_boost).clamp(0.0, 1.0);
                score.signals.knowledge_entities = knowledge_entities.clone();
                // Recompute level and model_tier based on boosted score
                score.level = if score.numeric < 0.35 {
                    loom_core::ComplexityLevel::Low
                } else if score.numeric < 0.7 {
                    loom_core::ComplexityLevel::Medium
                } else {
                    loom_core::ComplexityLevel::High
                };
                score.model_tier = match score.level {
                    loom_core::ComplexityLevel::Low => loom_core::ModelTier::Slm,
                    loom_core::ComplexityLevel::Medium => loom_core::ModelTier::Mid,
                    loom_core::ComplexityLevel::High => loom_core::ModelTier::Flagship,
                };
            }
            let decision = engine.decide(score.clone());
            let result = json!({
                "schema_version": 1,
                "command": "routing.evaluate",
                "complexity": score,
                "decision": decision,
                "knowledge_entities": knowledge_entities,
                "knowledge_boost": knowledge_boost,
            });
            if *json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!(
                    "Complexity: {:?} (numeric={:.2})",
                    score.level, score.numeric
                );
                if knowledge_boost > 0.0 {
                    println!(
                        "Knowledge context: {} entities found, boost=+{:.2}",
                        knowledge_entities.join(", "),
                        knowledge_boost
                    );
                }
                println!("Decision: {}", decision.reason);
                println!(
                    "  Model: {} ({})",
                    decision.recommended_model, decision.recommended_provider
                );
                println!("  Tier: {:?}", decision.recommended_tier);
                println!(
                    "  Cost: ${:.4} ({} input, {} output tokens)",
                    decision.cost_estimate.estimated_cost_usd,
                    decision.cost_estimate.input_tokens_estimated,
                    decision.cost_estimate.output_tokens_estimated
                );
            }
        }
        RoutingAction::Score { prompt, json } => {
            let mut score = classifier.classify(prompt);
            let (knowledge_entities, knowledge_boost) = knowledge_context(loom_dir, prompt);
            if knowledge_boost > 0.0 {
                score.numeric = (score.numeric + knowledge_boost).clamp(0.0, 1.0);
                score.signals.knowledge_entities = knowledge_entities.clone();
                score.level = if score.numeric < 0.35 {
                    loom_core::ComplexityLevel::Low
                } else if score.numeric < 0.7 {
                    loom_core::ComplexityLevel::Medium
                } else {
                    loom_core::ComplexityLevel::High
                };
                score.model_tier = match score.level {
                    loom_core::ComplexityLevel::Low => loom_core::ModelTier::Slm,
                    loom_core::ComplexityLevel::Medium => loom_core::ModelTier::Mid,
                    loom_core::ComplexityLevel::High => loom_core::ModelTier::Flagship,
                };
            }
            let result = json!({
                "schema_version": 1,
                "command": "routing.score",
                "complexity": score,
                "knowledge_entities": knowledge_entities,
                "knowledge_boost": knowledge_boost,
            });
            if *json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!(
                    "Complexity: {:?} (numeric={:.2})",
                    score.level, score.numeric
                );
                println!("Signals:");
                println!("  description_length: {}", score.signals.description_length);
                println!(
                    "  keyword_indicators: {:?}",
                    score.signals.keyword_indicators
                );
                println!(
                    "  has_cross_module_refs: {}",
                    score.signals.has_cross_module_refs
                );
                println!("  dependency_depth: {}", score.signals.dependency_depth);
                if let Some(count) = score.signals.scope_file_count {
                    println!("  scope_file_count: {}", count);
                }
                if let Some(role) = &score.signals.agent_role {
                    println!("  agent_role: {}", role);
                }
            }
        }
        RoutingAction::Recommend { task_id, json } => {
            let task = load_task(loom_dir, task_id)?;
            let score = task
                .complexity_score
                .clone()
                .unwrap_or_else(|| classifier.classify(&task.description));
            let decision = engine.decide(score.clone());
            let result = json!({
                "schema_version": 1,
                "command": "routing.recommend",
                "task_id": task.task_id,
                "complexity": score,
                "decision": decision,
            });
            if *json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Task: {} ({})", task.title, task.task_id);
                println!(
                    "Complexity: {:?} (numeric={:.2})",
                    score.level, score.numeric
                );
                println!(
                    "Recommendation: {} ({})",
                    decision.recommended_model, decision.recommended_provider
                );
                println!("  Tier: {:?}", decision.recommended_tier);
                println!("  Reason: {}", decision.reason);
                println!("  Cost: ${:.4}", decision.cost_estimate.estimated_cost_usd);
            }
        }
        RoutingAction::Override {
            task_id,
            tier,
            reason,
        } => {
            let mut task = load_task(loom_dir, task_id)?;
            let forced_tier = parse_tier(tier)?;

            // Ensure we have a complexity score
            let _score = if let Some(score) = task.complexity_score.clone() {
                score
            } else {
                classifier.classify(&task.description)
            };

            // Force tier override
            let catalog = CatalogManager::load(loom_dir.to_str().unwrap())?;
            let model = catalog
                .best_for_tier(&forced_tier)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("No model available for tier {:?}", forced_tier))?;

            let now = chrono::Utc::now().to_rfc3339();
            let input_tokens = 1000u32;
            let output_tokens = 500u32;
            let cost = (input_tokens as f64 / 1000.0) * model.input_cost_per_1k
                + (output_tokens as f64 / 1000.0) * model.output_cost_per_1k;

            let decision = RoutingDecision {
                recommended_tier: forced_tier,
                recommended_provider: model.provider_id.clone(),
                recommended_model: model.model_id.clone(),
                reason: reason
                    .clone()
                    .unwrap_or_else(|| format!("Manual override to {:?}", forced_tier)),
                overrides_role_default: true,
                decided_at: now.clone(),
                cost_estimate: loom_core::types::CostEstimate {
                    input_tokens_estimated: input_tokens,
                    output_tokens_estimated: output_tokens,
                    estimated_cost_usd: cost,
                    confidence: "manual".into(),
                },
            };

            task.routing_decision = Some(decision.clone());
            task.routed_provider = Some(model.provider_id.clone());
            task.updated_at = now;

            // Update the latest phase history entry if present
            if let Some(last) = task.phase_history.last_mut() {
                last.routing_decision = Some(decision.clone());
            }

            save_task(loom_dir, &task)?;

            println!(
                "Override applied to task {}: tier={:?}, model={} ({})",
                task_id, forced_tier, model.model_id, model.provider_id
            );
        }
        RoutingAction::Catalog { refresh, json } => {
            if *refresh {
                // Refresh catalog from all configured providers
                let _ = CatalogManager::refresh_from_providers(loom_dir.to_str().unwrap());
                println!("Catalog refreshed from providers.");
            }

            let catalog = CatalogManager::load(loom_dir.to_str().unwrap())?;
            if *json {
                let models: Vec<_> = catalog
                    .catalog()
                    .models
                    .iter()
                    .map(|m| {
                        json!({
                            "model_id": m.model_id,
                            "provider_id": m.provider_id,
                            "tier": format!("{:?}", m.tier),
                            "input_cost_per_1k": m.input_cost_per_1k,
                            "output_cost_per_1k": m.output_cost_per_1k,
                            "context_window": m.context_window,
                            "capabilities": m.capabilities,
                        })
                    })
                    .collect();
                println!(
                    "{}",
                    serde_json::to_string_pretty(&json!({
                        "schema_version": 1,
                        "command": "routing.catalog",
                        "refresh": *refresh,
                        "model_catalog": models
                    }))?
                );
            } else {
                println!(
                    "\nModel catalog ({} entries):",
                    catalog.catalog().models.len()
                );
                for m in &catalog.catalog().models {
                    println!(
                        "  {} @ {} tier={:?} ctx={}K ${:.4}/${:.4} per 1k",
                        m.model_id,
                        m.provider_id,
                        m.tier,
                        m.context_window / 1000,
                        m.input_cost_per_1k,
                        m.output_cost_per_1k
                    );
                }
            }
        }
    }
    Ok(())
}
