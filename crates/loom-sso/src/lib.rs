//! `loom-sso` — OIDC/SAML federation for the control plane.

pub mod config;
pub mod oidc;
pub mod saml;

pub use config::{SsoConfig, SsoMethod, AttributeMap};
pub use oidc::{OidcClient, OidcConfig, UserInfo};
pub use saml::{SamlSp, SamlConfig};