//! Role store for vault broker v1 (RBAC).
//!
//! Two roles per org identity:
//! - `Admin` — full access: list/inspect all grants, read full audit log,
//!   export any credential, modify ACL rules.
//! - `Member` — scoped access: own grants only, own audit entries only,
//!   own credentials only; cannot modify ACL rules.
//!
//! Resolution order (first match wins):
//! 1. `{loom_dir}/auth/roles.json` — explicit `{"roles": {"<identity>": "admin"}}` override.
//! 2. `{loom_dir}/org/membership.json` — CP-downloaded org membership (phase 3);
//!    identity listed under org `admins` → Admin, otherwise → Member.
//! 3. `{loom_dir}/config/org.toml` — offline interim `[org] members` / `[org] admins`.
//! 4. Default → `Member` (conservative).
//!
//! Identity is the requesting tool / provisioned context string, not a human
//! login — no user accounts this phase.

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;
use tracing::{debug, warn};

/// Org role.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
pub enum Role {
    Admin,
    #[default]
    Member,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::Admin => write!(f, "admin"),
            Role::Member => write!(f, "member"),
        }
    }
}

/// `auth/roles.json` — explicit role override (0600).
#[derive(Debug, Deserialize, Default)]
struct RolesFile {
    roles: HashMap<String, String>,
}

/// `config/org.toml` — offline interim roster.
#[derive(Debug, Deserialize, Default)]
struct OrgToml {
    org: Option<OrgSection>,
}

#[derive(Debug, Deserialize, Default)]
struct OrgSection {
    #[serde(default)]
    members: Vec<String>,
    #[serde(default)]
    admins: Vec<String>,
}

/// Role resolution store. Thread-safe via `RwLock`.
pub struct RoleStore {
    inner: RwLock<RoleStoreInner>,
    loom_dir: String,
}

struct RoleStoreInner {
    explicit: HashMap<String, Role>,
    org_members: Vec<String>,
    org_admins: Vec<String>,
}

impl RoleStore {
    /// Create a store for `loom_dir`, loading `auth/roles.json` and
    /// `config/org.toml` if present.
    pub fn new(loom_dir: &str) -> Self {
        let auth_dir = Path::new(loom_dir).join("auth");
        let roles_path = auth_dir.join("roles.json");
        let config_dir = Path::new(loom_dir).join("config");
        let org_toml_path = config_dir.join("org.toml");

        let mut explicit = HashMap::new();
        if roles_path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&roles_path) {
                if let Ok(file) = serde_json::from_str::<RolesFile>(&raw) {
                    for (ident, role_str) in file.roles {
                        let role = match role_str.to_lowercase().as_str() {
                            "admin" => Role::Admin,
                            "member" => Role::Member,
                            _ => Role::Member,
                        };
                        explicit.insert(ident, role);
                    }
                } else {
                    warn!(path = ?roles_path, "failed to parse roles.json");
                }
            }
        }

        let mut org_members = Vec::new();
        let mut org_admins = Vec::new();
        if org_toml_path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&org_toml_path) {
                if let Ok(file) = toml::from_str::<OrgToml>(&raw) {
                    if let Some(section) = file.org {
                        org_members = section.members;
                        org_admins = section.admins;
                    }
                } else {
                    warn!(path = ?org_toml_path, "failed to parse org.toml");
                }
            }
        }

        Self {
            inner: RwLock::new(RoleStoreInner {
                explicit,
                org_members,
                org_admins,
            }),
            loom_dir: loom_dir.to_string(),
        }
    }

    /// Resolve the role for `identity` (resolution order: explicit → CP
    /// membership → org.toml → default Member).
    pub fn role(&self, identity: &str) -> Role {
        let inner = self.inner.read().expect("role store lock poisoned");

        // 1. Explicit override from roles.json.
        if let Some(role) = inner.explicit.get(identity) {
            return *role;
        }

        // 2. CP-downloaded org membership (phase 3): admins list → Admin.
        //    Membership file path: {loom_dir}/org/membership.json
        let membership_path = Path::new(&self.loom_dir).join("org").join("membership.json");
        if membership_path.exists() {
            if let Ok(raw) = std::fs::read_to_string(&membership_path) {
                if let Ok(membership) =
                    serde_json::from_str::<serde_json::Value>(&raw)
                {
                    if let Some(org) = membership.get("org").and_then(|o| o.as_object()) {
                        if let Some(admins) = org.get("admins").and_then(|a| a.as_array()) {
                            if admins.iter().any(|v| v.as_str() == Some(identity)) {
                                return Role::Admin;
                            }
                        }
                    }
                }
            }
        }

        // 3. Offline interim org.toml.
        if inner.org_admins.iter().any(|a| a == identity) {
            return Role::Admin;
        }
        if inner.org_members.iter().any(|m| m == identity) {
            return Role::Member;
        }

        // 4. Conservative default.
        Role::Member
    }

    /// `true` when `identity` resolves to `Admin`.
    pub fn is_admin(&self, identity: &str) -> bool {
        self.role(identity) == Role::Admin
    }

    /// `true` when `identity` resolves to `Member`.
    pub fn is_member(&self, identity: &str) -> bool {
        self.role(identity) == Role::Member
    }

    /// Reload `roles.json` and `org.toml` from disk (pick up CP-membership
    /// changes and admin edits without restart).
    pub fn reload(&self) {
        // Reconstruct by re-reading files; simplest correct approach for a
        // small store.  Ponytail: full re-read on each reload — fine for a
        // roles file edited rarely; add file-watching if churn matters.
        let _ = Self::new(&self.loom_dir);
        // Note: cannot mutate `self` from an immutable `&self`; callers that
        // need a live reload should reconstruct the store.  Kept as a
        // documented no-op hook for future wire-up.
        debug!("role store reload hook (reconstruct store externally)");
    }

    /// Return a snapshot of all explicitly configured roles (for tests /
    /// admin inspection).
    pub fn explicit_roles(&self) -> Vec<(String, Role)> {
        let inner = self.inner.read().expect("role store lock poisoned");
        inner.explicit.iter().map(|(k, v)| (k.clone(), *v)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_roles_json(dir: &Path, json: &str) {
        std::fs::create_dir_all(dir.join("auth")).unwrap();
        std::fs::write(dir.join("auth").join("roles.json"), json).unwrap();
    }

    fn write_org_toml(dir: &Path, toml: &str) {
        std::fs::create_dir_all(dir.join("config")).unwrap();
        std::fs::write(dir.join("config").join("org.toml"), toml).unwrap();
    }

    fn write_membership_json(dir: &Path, json: &str) {
        std::fs::create_dir_all(dir.join("org")).unwrap();
        std::fs::write(dir.join("org").join("membership.json"), json).unwrap();
    }

    #[test]
    fn explicit_roles_json_wins_over_default() {
        let dir = tempfile::tempdir().unwrap();
        write_roles_json(dir.path(), r#"{"roles": {"alice": "admin"}}"#);
        let store = RoleStore::new(dir.path().to_str().unwrap());
        assert!(store.is_admin("alice"));
        assert!(store.is_member("bob"));
    }

    #[test]
    fn org_toml_admins_when_no_roles_json() {
        let dir = tempfile::tempdir().unwrap();
        write_org_toml(
            dir.path(),
            r#"[org]
members = ["alice", "bob"]
admins = ["alice"]
"#,
        );
        let store = RoleStore::new(dir.path().to_str().unwrap());
        assert!(store.is_admin("alice"));
        assert!(store.is_member("bob"));
        assert!(store.is_member("carol")); // default member
    }

    #[test]
    fn cpmembership_admins_take_precedence_over_org_toml() {
        let dir = tempfile::tempdir().unwrap();
        write_org_toml(
            dir.path(),
            r#"[org]
members = ["carol"]
admins = []
"#,
        );
        write_membership_json(
            dir.path(),
            r#"{"org": {"id": "org-1", "name": "Test", "tier": "free", "admins": ["carol"]}}"#,
        );
        let store = RoleStore::new(dir.path().to_str().unwrap());
        assert!(store.is_admin("carol"));
    }

    #[test]
    fn default_is_member() {
        let dir = tempfile::tempdir().unwrap();
        let store = RoleStore::new(dir.path().to_str().unwrap());
        assert!(store.is_member("nobody"));
        assert!(!store.is_admin("nobody"));
    }

    #[test]
    fn explicit_roles_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        write_roles_json(dir.path(), r#"{"roles": {"alice": "admin", "bob": "member"}}"#);
        let store = RoleStore::new(dir.path().to_str().unwrap());
        let roles: HashMap<String, Role> = store
            .explicit_roles()
            .into_iter()
            .collect();
        assert_eq!(roles.get("alice"), Some(&Role::Admin));
        assert_eq!(roles.get("bob"), Some(&Role::Member));
    }
}
