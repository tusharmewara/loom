//! OIDC client for the control plane.

use anyhow::{Context, Result};
use openidconnect::core::{
    CoreAuthenticationFlow, CoreClient, CoreIdTokenClaims, CoreJsonWebKeySet, CoreProviderMetadata,
    CoreResponseType,
};
use openidconnect::{
    AccessTokenHash, AuthorizationCode, ClientId, ClientSecret, CsrfToken, IssuerUrl, Nonce, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};
use std::sync::Arc;
use url::Url;

use crate::config::{OidcConfig, UserInfo};

#[derive(Clone)]
pub struct OidcClient {
    client: Arc<CoreClient>,
    config: OidcConfig,
}

impl OidcClient {
    pub async fn new(config: OidcConfig) -> Result<Self> {
        let issuer_url = IssuerUrl::new(config.issuer_url.clone())
            .context("invalid issuer URL")?;
        // SSRF guard: reject private/loopback issuer URLs before discovery
        // follows redirects. Fail-closed.
        loom_core::plugin_registry::validate_outbound_url(&config.issuer_url)
            .with_context(|| format!("issuer URL rejected: {}", config.issuer_url))?;
        let provider_metadata = CoreProviderMetadata::discover_async(issuer_url, openidconnect::reqwest::async_http_client)
            .await
            .context("failed to discover OIDC provider metadata")?;

        let client = CoreClient::from_provider_metadata(
            provider_metadata,
            ClientId::new(config.client_id.clone()),
            Some(ClientSecret::new(config.client_secret.clone())),
        )
        .set_redirect_uri(RedirectUrl::new("http://127.0.0.1:7333/api/v1/oidc/callback".into())
            .context("invalid redirect URL")?);

        Ok(Self {
            client: Arc::new(client),
            config,
        })
    }

    pub fn authorize_url(&self) -> (Url, CsrfToken, PkceCodeVerifier) {
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();
        let (auth_url, csrf_token, _nonce) = self.client
            .authorize_url(
                CoreAuthenticationFlow::AuthorizationCode,
                CsrfToken::new_random,
                Nonce::new_random,
            )
            .add_scope(Scope::new("openid".into()))
            .add_scope(Scope::new("email".into()))
            .add_scope(Scope::new("profile".into()))
            .set_pkce_challenge(pkce_challenge)
            .url();
        (auth_url, csrf_token, pkce_verifier)
    }

    pub async fn exchange_code(&self, code: String, pkce_verifier: PkceCodeVerifier) -> Result<UserInfo> {
        let token_response = self.client
            .exchange_code(AuthorizationCode::new(code))
            .set_pkce_verifier(pkce_verifier)
            .request_async(openidconnect::reqwest::async_http_client)
            .await
            .context("failed to exchange code for tokens")?;

        let id_token = token_response.id_token()
            .context("no ID token in response")?;
        let claims = id_token.claims(&self.client.id_token_verifier(), &Nonce::new_random())
            .context("failed to verify ID token")?;

        let email = claims.email()
            .context("email claim missing")?
            .to_string();
        let name = claims.name()
            .and_then(|n| n.get(None))
            .map(|n| n.to_string())
            .unwrap_or_else(|| email.clone());

        let groups = claims
            .claim(&self.config.attribute_map.groups_claim.clone().unwrap_or("groups".into()))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        Ok(UserInfo { email, name, groups })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    #[tokio::test]
    async fn oidc_discovery_and_authorize_url() {
        let mut server = Server::new_async().await;
        let _m = server.mock("GET", "/.well-known/openid-configuration")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{
                "issuer": "http://mock-idp",
                "authorization_endpoint": "http://mock-idp/authorize",
                "token_endpoint": "http://mock-idp/token",
                "jwks_uri": "http://mock-idp/jwks",
                "response_types_supported": ["code"],
                "subject_types_supported": ["public"],
                "id_token_signing_alg_values_supported": ["RS256"]
            }"#)
            .create_async()
            .await;

        let config = OidcConfig {
            issuer_url: server.url(),
            client_id: "test-client".into(),
            client_secret: "test-secret".into(),
            attribute_map: Default::default(),
        };

        let client = OidcClient::new(config).await.unwrap();
        let (url, _csrf, _pkce) = client.authorize_url();
        assert!(url.to_string().contains("http://mock-idp/authorize"));
    }

    #[tokio::test]
    async fn oidc_issuer_rejects_private_url() {
        use loom_core::plugin_registry::validate_outbound_url;

        for bad in [
            "https://127.0.0.1/",
            "https://169.254.169.254/latest/meta-data/",
            "https://[::1]/",
            "https://10.0.0.5/",
        ] {
            let err = validate_outbound_url(bad).unwrap_err();
            assert!(
                err.to_string().contains("private/loopback"),
                "expected block for {bad}, got: {err}"
            );
        }
        // Public issuers pass validation
        validate_outbound_url("https://accounts.example.com/").unwrap();
    }
}