// WASM bindings for Loom
#[cfg(target_arch = "wasm32")]
pub mod wasm {
    use wasm_bindgen::prelude::*;

    #[wasm_bindgen]
    pub fn version() -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }

    /// Classify a prompt and return the complexity score as a JSON string.
    #[wasm_bindgen]
    pub fn classify_prompt(prompt: &str) -> String {
        let engine = loom_routing::RoutingEngine::new("");
        let score = engine.classify(prompt);
        serde_json::to_string(&score)
            .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
    }

    /// Resolve a full routing decision for a prompt, returning JSON.
    #[wasm_bindgen]
    pub fn resolve_route(prompt: &str) -> String {
        let engine = loom_routing::RoutingEngine::new("");
        let score = engine.classify(prompt);
        let decision = engine.decide(score);
        serde_json::to_string(&decision)
            .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
    }
}
