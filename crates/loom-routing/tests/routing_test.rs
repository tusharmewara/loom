use loom_routing::{score_static, CatalogManager, Classifier, DecisionEngine};

#[test]
fn test_score_static_low_complexity() {
    let score = score_static("fix typo in readme");
    assert!(score.numeric < 0.5);
}

#[test]
fn test_score_static_high_complexity() {
    let score = score_static("refactor the distributed security architecture across modules");
    assert!(score.numeric >= 0.5);
}

#[test]
fn test_router_picks_model() {
    let classifier = Classifier::new();
    let engine = DecisionEngine::new();
    let score = classifier.classify("implement auth middleware");
    let decision = engine.decide(score);
    assert!(!decision.recommended_model.is_empty());
    assert!(decision.cost_estimate.estimated_cost_usd >= 0.0);
    // A routing decision is a forecast, never a measurement.
    assert_eq!(
        decision.cost_estimate.source,
        loom_core::UsageSource::Estimated,
        "a decision taken before the run cannot be a measured cost"
    );
    assert_eq!(decision.cost_estimate.method, "chars/4");
}

#[test]
fn test_cost_estimate_grows_with_the_prompt() {
    let classifier = Classifier::new();
    let engine = DecisionEngine::new();

    let small = engine.decide(classifier.classify("fix a typo"));
    let large = engine.decide(classifier.classify(&"refactor the whole module. ".repeat(200)));

    assert!(
        large.cost_estimate.input_tokens_estimated > small.cost_estimate.input_tokens_estimated,
        "a 4400-character prompt must not cost the same as a 15-character one"
    );
    assert!(
        large.cost_estimate.estimated_cost_usd > small.cost_estimate.estimated_cost_usd,
        "the estimate must follow the prompt size"
    );
    // The estimate is derived from the scored prompt, not a fixed constant.
    assert_ne!(
        small.cost_estimate.input_tokens_estimated, 1000,
        "the old fixed 1000-token constant must be gone"
    );
}

#[test]
fn test_cost_estimate_is_computed_from_the_token_count() {
    let classifier = Classifier::new();
    let engine = DecisionEngine::new();
    let prompt = "a".repeat(400);
    let decision = engine.decide(classifier.classify(&prompt));

    // chars/4 for 400 characters is 100 input tokens.
    assert_eq!(decision.cost_estimate.input_tokens_estimated, 100);
    assert_eq!(decision.cost_estimate.output_tokens_estimated, 25);

    let catalog = CatalogManager::default_catalog();
    let model = catalog
        .models
        .iter()
        .find(|m| m.model_id == decision.recommended_model)
        .expect("the decided model must be in the catalog");    let expected = (100.0 / 1000.0) * model.input_cost_per_1k
        + (25.0 / 1000.0) * model.output_cost_per_1k;
    assert!((decision.cost_estimate.estimated_cost_usd - expected).abs() < 1e-12);
}

#[test]
fn test_default_catalog_loads() {
    let catalog = CatalogManager::default_catalog();
    assert!(!catalog.models.is_empty());
}

#[test]
fn test_prices_for_finds_a_dated_model_id() {
    let catalog = CatalogManager::default_catalog();
    let (input, output) = catalog.prices_for("claude-sonnet-4-20250514").unwrap();
    assert!(input > 0.0 && output > 0.0);
    assert!(catalog.prices_for("no-such-model").is_none());
}