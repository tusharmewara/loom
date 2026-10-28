//! SSO configuration types.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "lowercase")]
pub enum SsoConfig {
    Oidc(OidcConfig),
    Saml(SamlConfig),
}

impl SsoConfig {
    pub fn method(&self) -> SsoMethod {
        match self {
            SsoConfig::Oidc(_) => SsoMethod::Oidc,
            SsoConfig::Saml(_) => SsoMethod::Saml,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SsoMethod {
    Oidc,
    Saml,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcConfig {
    pub issuer_url: String,
    pub client_id: String,
    #[serde(skip_serializing)]
    pub client_secret: String,
    pub attribute_map: AttributeMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamlConfig {
    pub entity_id: String,
    pub sso_url: String,
    pub slo_url: Option<String>,
    #[serde(skip_serializing)]
    pub x509_cert: String,
    pub attribute_map: AttributeMap,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeMap {
    pub email_claim: String,
    pub name_claim: String,
    pub groups_claim: Option<String>,
}

impl Default for AttributeMap {
    fn default() -> Self {
        Self {
            email_claim: "email".into(),
            name_claim: "name".into(),
            groups_claim: Some("groups".into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserInfo {
    pub email: String,
    pub name: String,
    pub groups: Vec<String>,
}