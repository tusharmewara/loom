//! Topic derivation and filtering for events.
//!
//! Maps [`LoomEvent`] variants to string topics for routing and
//! cross-process subscription filtering.

use loom_core::LoomEvent;

/// A topic string identifying the category of an event.
///
/// Topics follow the `{domain}:{action}` convention used in the spec:
/// - `skills:added`
/// - `mcp:health-changed`
/// - `session:exported`
/// - etc.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Topic(String);

impl Topic {
    /// Derive the topic from a `LoomEvent` variant.
    pub fn from_event(event: &LoomEvent) -> Self {
        match event {
            LoomEvent::SkillsAdded { .. } => Self("skills:added".into()),
            LoomEvent::SkillsRemoved { .. } => Self("skills:removed".into()),
            LoomEvent::McpHealthChanged { .. } => Self("mcp:health-changed".into()),
            LoomEvent::McpServerAdded { .. } => Self("mcp:server-added".into()),
            LoomEvent::SessionExported { .. } => Self("session:exported".into()),
            LoomEvent::RoutingUpdated => Self("routing:updated".into()),
            LoomEvent::AuthProviderAdded { .. } => Self("auth:provider-added".into()),
            LoomEvent::HooksRegistered { .. } => Self("hooks:registered".into()),
            LoomEvent::TaskPhaseChanged { .. } => Self("task:phase-changed".into()),
            LoomEvent::TaskCreated { .. } => Self("task:created".into()),
        }
    }

    /// Create a topic from a string (e.g. for config-based subscription).
    ///
    /// Returns `None` if the string doesn't match a known event topic.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "skills:added"
            | "skills:removed"
            | "mcp:health-changed"
            | "mcp:server-added"
            | "session:exported"
            | "routing:updated"
            | "auth:provider-added"
            | "hooks:registered"
            | "task:phase-changed"
            | "task:created" => Some(Self(s.to_string())),
            _ => None,
        }
    }

    /// Return the topic as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Check if this topic matches a given topic.
    pub fn matches(&self, other: &Topic) -> bool {
        self.0 == other.0
    }

    /// All known event topics (useful for introspection).
    pub fn all() -> Vec<&'static str> {
        vec![
            "skills:added",
            "skills:removed",
            "mcp:health-changed",
            "mcp:server-added",
            "session:exported",
            "routing:updated",
            "auth:provider-added",
            "hooks:registered",
            "task:phase-changed",
            "task:created",
        ]
    }
}

impl std::str::FromStr for Topic {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(())
    }
}

impl std::fmt::Display for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
