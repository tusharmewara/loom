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
}

#[test]
fn test_default_catalog_loads() {
    let catalog = CatalogManager::default_catalog();
    assert!(!catalog.models.is_empty());
}
