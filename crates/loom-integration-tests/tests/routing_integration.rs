//! Integration tests for Intelligent Routing (Trellis alignment)
//! Covers: complexity scoring, config thresholds, phase transitions, cost optimization, overrides.

use anyhow::Result;
use loom_core::{
    config::ConfigReader,
    types::{ComplexityLevel, ModelTier, RoutingDecision},
};
use loom_routing::{Classifier, DecisionEngine};
use loom_workflow::task_manager::TaskManager;
use std::fs;
use tempfile::tempdir;

fn loom_dir(temp: &tempfile::TempDir) -> String {
    temp.path().join("agents").to_string_lossy().into()
}

/// Write a minimal Loom config with custom routing thresholds.
fn write_routing_config(loom_dir: &str, strategy: &str, low_max: f64, high_min: f64) -> Result<()> {
    let config_dir = std::path::Path::new(loom_dir).join("config");
    fs::create_dir_all(&config_dir)?;
    let content = format!(
        r#"strategy = "{}"
[complexity_thresholds]
low_max = {}
high_min = {}
"#,
        strategy, low_max, high_min
    );
    fs::write(config_dir.join("routing.toml"), content)?;
    Ok(())
}

/// Ensure default model catalog exists (CatalogManager creates it if missing).
fn ensure_catalog(loom_dir: &str) -> Result<()> {
    let _ = loom_routing::CatalogManager::load(loom_dir)?;
    Ok(())
}

#[test]
fn test_complexity_scoring_basic() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    ensure_catalog(&agents)?;

    let classifier = Classifier::new();

    // Low complexity task
    let low_prompt = "Fix typo in README: change 'teh' to 'the'.";
    let low_score = classifier.classify(low_prompt);
    assert!(matches!(low_score.level, ComplexityLevel::Low));
    assert!(low_score.numeric < 0.4);

    // High complexity task
    let high_prompt = "We need to architect a distributed system to handle millions of users across multiple regions. This will involve redesigning our API gateway, implementing a new authentication service, and ensuring cross-module consistency. Performance and security are critical.";
    let high_score = classifier.classify(high_prompt);
    assert!(matches!(
        high_score.level,
        ComplexityLevel::Medium | ComplexityLevel::High
    ));
    assert!(high_score.numeric > low_score.numeric);

    Ok(())
}

#[test]
fn test_routing_uses_config_thresholds() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    write_routing_config(&agents, "Balanced", 0.3, 0.6)?; // thresholds
    ensure_catalog(&agents)?;

    let config = ConfigReader::new(&agents).load()?.routing;
    let engine = DecisionEngine::with_loom_dir(&agents, config)?;

    // Simulate a medium numeric score (0.5) -> should map to Mid tier
    let score = loom_core::types::ComplexityScore {
        level: ComplexityLevel::Medium,
        numeric: 0.5,
        model_tier: ModelTier::Flagship, // initial, may be overridden
        scored_at: "2026-05-17T00:00:00Z".to_string(),
        signals: Default::default(),
    };
    let decision = engine.decide(score.clone());
    assert!(matches!(decision.recommended_tier, ModelTier::Mid));

    // Low numeric 0.2 -> Slm
    let score_low = loom_core::types::ComplexityScore {
        numeric: 0.2,
        ..score.clone()
    };
    let decision_low = engine.decide(score_low);
    assert!(matches!(decision_low.recommended_tier, ModelTier::Slm));

    // High numeric 0.8 -> Flagship
    let score_high = loom_core::types::ComplexityScore {
        numeric: 0.8,
        ..score
    };
    let decision_high = engine.decide(score_high);
    assert!(matches!(
        decision_high.recommended_tier,
        ModelTier::Flagship
    ));

    Ok(())
}

#[test]
fn test_threshold_boundaries_and_contradiction_case() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    write_routing_config(&agents, "Balanced", 0.3, 0.7)?; // non-default cutoffs
    ensure_catalog(&agents)?;

    let config = ConfigReader::new(&agents).load()?.routing;
    let engine = DecisionEngine::with_loom_dir(&agents, config.clone())?;
    let thresholds = config
        .as_ref()
        .map(|c| c.complexity_thresholds.clone())
        .unwrap_or_default();

    // routing-debt Step 3 contradiction case: numeric=0.32 with
    // low_max=0.3 used to yield level=Low (hardcoded 0.35 cutoff in the
    // scorer/CLI) but tier=Mid (config cutoff in the router). Everything
    // now derives through derive_level/derive_tier: Medium/Mid, consistent.
    let score = loom_core::types::ComplexityScore {
        level: ComplexityLevel::Low, // stale default-thresholds level on purpose
        numeric: 0.32,
        model_tier: ModelTier::Slm, // stale too: decide() re-derives
        scored_at: "2026-05-17T00:00:00Z".to_string(),
        signals: Default::default(),
    };
    let level = loom_core::derive_level(0.32, &thresholds);
    assert!(matches!(level, ComplexityLevel::Medium));
    let decision = engine.decide(score);
    assert_eq!(decision.recommended_tier, loom_core::derive_tier(level));

    // Boundary semantics (strict <): exactly low_max -> Mid, exactly
    // high_min -> Flagship; just below low_max -> Slm.
    let mk_score = |numeric| loom_core::types::ComplexityScore {
        level: loom_core::derive_level(numeric, &thresholds),
        numeric,
        model_tier: ModelTier::Slm,
        scored_at: "2026-05-17T00:00:00Z".to_string(),
        signals: Default::default(),
    };
    assert!(matches!(
        engine.decide(mk_score(0.29)).recommended_tier,
        ModelTier::Slm
    ));
    assert!(matches!(
        engine.decide(mk_score(0.3)).recommended_tier,
        ModelTier::Mid
    ));
    assert!(matches!(
        engine.decide(mk_score(0.69)).recommended_tier,
        ModelTier::Mid
    ));
    assert!(matches!(
        engine.decide(mk_score(0.7)).recommended_tier,
        ModelTier::Flagship
    ));

    Ok(())
}

#[test]
fn test_phase_transition_records_dynamic_signals() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    ensure_catalog(&agents)?;
    let manager = TaskManager::new(&agents);

    // Create a task
    let task_id = {
        let task = manager.create_task(
            "Implement feature X",
            "Add a new endpoint /v1/feature that does something useful across modules.",
            false,
            None,
        )?;
        task.task_id
    };

    // Advance from Plan -> Implement
    let task_after = manager.advance_phase(&task_id)?;

    // Check phase history: last entry should have complexity_score and routing_decision
    let last_phase = task_after
        .phase_history
        .last()
        .ok_or_else(|| anyhow::anyhow!("no phase history"))?;
    assert!(last_phase.complexity_score.is_some());
    assert!(last_phase.routing_decision.is_some());

    // Ensure the phase entry corresponds to Implement
    assert_eq!(last_phase.phase, "Implement");

    // The task-level complexity_score should also be set
    assert!(task_after.complexity_score.is_some());
    assert!(task_after.routing_decision.is_some());

    // Check dynamic signals: since we just advanced from Plan->Implement, files_modified_count should be 0 initially
    // Directly check signals from the complexity score
    let cs = task_after.complexity_score.as_ref().unwrap();
    assert_eq!(cs.signals.files_modified_count, Some(0)); // no files modified yet
                                                          // phase_iteration_count should be 1 (first time in Implement)
    assert_eq!(cs.signals.phase_iteration_count, Some(1));

    Ok(())
}

#[test]
fn test_cost_optimization_savings() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    // Use CostOptimize strategy to push low to SLM and mid to Mid, but high will still be Flagship
    write_routing_config(&agents, "CostOptimize", 0.3, 0.6)?;
    ensure_catalog(&agents)?;

    let config = ConfigReader::new(&agents).load()?.routing;
    let classifier = Classifier::new();
    let engine = DecisionEngine::with_loom_dir(&agents, config)?;

    // Define three prompts that will yield Low, Medium, High after scoring
    let prompts = vec![
        ("low", "Fix typo"), // should be Low
        ("medium", "Refactor utils module to improve readability."), // Medium
        ("high", "Design and implement a distributed microservices architecture with cross-module consistency, ensuring high performance and security."), // High
    ];

    let mut total_cost_tiered = 0.0;
    let mut total_cost_all_flagship = 0.0;

    for (_label, prompt) in prompts {
        let score = classifier.classify(prompt);
        let decision = engine.decide(score.clone());
        let cost = decision.cost_estimate.estimated_cost_usd;
        total_cost_tiered += cost;

        // Compute all-flagship cost: use Flagship tier with same tokens
        // We approximate: for each model in catalog, get Flagship model cost
        let flagship_cost = {
            let catalog = loom_routing::CatalogManager::load(&agents)?;
            let model = catalog
                .best_for_tier(&ModelTier::Flagship)
                .ok_or_else(|| anyhow::anyhow!("no flagship model"))?;
            let in_cost = model.input_cost_per_1k;
            let out_cost = model.output_cost_per_1k;
            let input_tokens = decision.cost_estimate.input_tokens_estimated as f64 / 1000.0;
            let output_tokens = decision.cost_estimate.output_tokens_estimated as f64 / 1000.0;
            input_tokens * in_cost + output_tokens * out_cost
        };
        total_cost_all_flagship += flagship_cost;
    }

    println!("Total tiered cost: ${:.4}", total_cost_tiered);
    println!("Total all-flagship cost: ${:.4}", total_cost_all_flagship);
    assert!(total_cost_tiered <= total_cost_all_flagship + f64::EPSILON);
    Ok(())
}

#[test]
fn test_manual_override_persists() -> Result<()> {
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    ensure_catalog(&agents)?;
    let manager = TaskManager::new(&agents);

    // Create a task
    let task = manager.create_task("Override test", "A simple task", false, None)?;
    let task_id = task.task_id;

    // Simulate manual override: directly set routing decision to Slm
    let catalog = loom_routing::CatalogManager::load(&agents)?;
    let model = catalog
        .best_for_tier(&ModelTier::Slm)
        .ok_or_else(|| anyhow::anyhow!("no SLM model"))?;
    let override_decision = RoutingDecision {
        recommended_tier: ModelTier::Slm,
        recommended_provider: model.provider_id.clone(),
        recommended_model: model.model_id.clone(),
        reason: "Manual override".to_string(),
        overrides_role_default: true,
        decided_at: "2026-05-17T00:00:00Z".to_string(),
        cost_estimate: loom_core::types::CostEstimate {
            input_tokens_estimated: 1000,
            output_tokens_estimated: 500,
            estimated_cost_usd: (1.0 * model.input_cost_per_1k + 0.5 * model.output_cost_per_1k),
            confidence: "manual".into(),
        },
    };

    // Load, modify, and save task
    let mut task = manager.get(&task_id)?.unwrap();
    task.routing_decision = Some(override_decision.clone());
    task.routed_provider = Some(model.provider_id.clone());
    let task_path = std::path::Path::new(&agents)
        .join("workflow")
        .join("tasks")
        .join(format!("{task_id}.json"));
    fs::write(task_path, serde_json::to_string_pretty(&task)?)?;

    // Reload and verify
    let reloaded = manager.get(&task_id)?;
    let reloaded = reloaded
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("task missing"))?;
    assert!(reloaded.routing_decision.is_some());
    let rd = reloaded.routing_decision.as_ref().unwrap();
    assert!(matches!(rd.recommended_tier, ModelTier::Slm));
    assert!(rd.overrides_role_default);
    assert_eq!(
        reloaded.routed_provider.as_ref().unwrap(),
        &model.provider_id
    );

    Ok(())
}

#[test]
fn test_routing_outcomes_recorded_and_closed() -> Result<()> {
    // Persisted (signals → decision → outcome) telemetry
    // (routing-debt.md Step 6): decisions from create_task/advance_phase
    // land in the store, and completion closes the task's open rows.
    let dir = tempdir()?;
    let agents = loom_dir(&dir);
    ensure_catalog(&agents)?;
    let manager = TaskManager::new(&agents);

    let task = manager.create_task(
        "Telemetry test",
        "Add a new endpoint /v1/feature that does something useful across modules.",
        false,
        None,
    )?;
    let task_id = task.task_id;

    let store = loom_routing::RoutingOutcomeStore::new(&agents)?;
    let rows = store.recent_decisions(10)?;
    assert_eq!(rows.len(), 1, "create_task records one decision");
    assert_eq!(rows[0].source, "create_task");
    assert_eq!(rows[0].task_id, task_id);
    assert_eq!(rows[0].phase.as_deref(), Some("Plan"));
    assert_eq!(
        rows[0].tier,
        format!(
            "{:?}",
            task.routing_decision.as_ref().unwrap().recommended_tier
        )
    );
    assert!(rows[0].numeric >= 0.0);
    assert!(serde_json::from_str::<serde_json::Value>(&rows[0].signals_json).is_ok());

    // Each phase advance records another decision.
    manager.advance_phase(&task_id)?; // Implement
    manager.advance_phase(&task_id)?; // Verify
    assert_eq!(store.recent_decisions(10)?.len(), 3);

    // Manual override closes the latest open decision as overridden.
    let overridden = store.record_override(&task_id, "Mid", "Flagship")?;
    assert_eq!(overridden, 1);
    let rows = store.recent_decisions(10)?;
    let latest = rows
        .iter()
        .find(|r| r.phase.as_deref() == Some("Verify"))
        .unwrap();
    assert_eq!(latest.outcome.as_deref(), Some("overridden"));
    assert!(latest
        .outcome_detail
        .as_deref()
        .unwrap()
        .contains("Flagship"));

    // Finish → completion closes every remaining open row.
    manager.advance_phase(&task_id)?; // Finish
    let rows = store.recent_decisions(10)?;
    assert_eq!(rows.len(), 4);
    assert!(
        rows.iter().all(|r| r.outcome.is_some()),
        "no row left open after completion: {:?}",
        rows.iter().map(|r| r.outcome.clone()).collect::<Vec<_>>()
    );
    let counts = store.outcome_counts()?;
    assert_eq!(
        counts,
        vec![("completed".to_string(), 3), ("overridden".to_string(), 1)]
    );

    Ok(())
}
