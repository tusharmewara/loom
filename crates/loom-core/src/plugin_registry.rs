use crate::PluginManifest;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, ToSocketAddrs};
use std::path::{Path, PathBuf};

/// Validate that a URL is safe for outbound fetching: HTTPS scheme only,
/// and the host must not resolve to a private or loopback IP address.
///
/// Blocks RFC 1918 (10/8, 172.16/12, 192.168/16), loopback (127/8, ::1),
/// link-local (169.254/16, fe80::/10), unique-local (fc00::/7), multicast,
/// reserved, and IPv4-mapped IPv6 forms.  Decimal/octal/hex IP literals
/// (e.g. `2130706433`, `0177.0.0.1`) are not parseable by `IpAddr`, so
/// they fall through to DNS lookup and fail there — not a practical SSRF
/// vector against modern HTTP clients.
///
/// Fail-closed: returns `Err` on any non-HTTPS or private/loopback host.
/// Callers must treat the error as fatal (no silent-skip).
pub fn validate_outbound_url(url: &str) -> Result<()> {
    validate_https_scheme(url)?;
    let host = extract_host(url).ok_or_else(|| {
        anyhow::anyhow!("invalid URL: no host in '{url}'")
    })?;

    if host.is_empty() {
        anyhow::bail!("refusing outbound URL with empty host in '{url}'");
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        if is_private_or_loopback(ip) {
            anyhow::bail!(
                "refusing outbound URL to private/loopback host '{host}' in '{url}'"
            );
        }
        return Ok(());
    }

    let resolved = (host, 443)
        .to_socket_addrs()
        .map_err(|_| anyhow::anyhow!("invalid URL host '{host}' in '{url}'"))?;
    for addr in resolved {
        if is_private_or_loopback(addr.ip()) {
            anyhow::bail!(
                "refusing outbound URL to private/loopback host '{host}' in '{url}'"
            );
        }
    }

    Ok(())
}

/// HTTPS scheme check only (kept for callers that don't need IP blocking).
fn validate_https_scheme(url: &str) -> Result<()> {
    if !url.starts_with("https://") {
        anyhow::bail!(
            "refusing non-HTTPS URL '{url}': outbound fetches are HTTPS-only"
        );
    }
    Ok(())
}

/// Extract the host portion from an `https://` URL.
/// Handles `[ipv6]:port`, `host:port`, and bare `host`.
fn extract_host(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
let end = rest.find(|c| c == '/' || c == '?' || c == '#').unwrap_or(rest.len());
    let host_port = &rest[..end];
    if let Some(bracketed) = host_port.strip_prefix('[') {
        return bracketed.split(']').next();
    }
    host_port.split(':').next()
}

/// Check whether an IP address is private, loopback, link-local, or
/// otherwise non-routable on the public internet.
fn is_private_or_loopback(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let o = v4.octets();
            // 0.0.0.0/8, 10.0.0.0/8, 127.0.0.0/8 loopback
            if o[0] == 0 || o[0] == 10 || o[0] == 127 {
                return true;
            }
            // 169.254.0.0/16 link-local (AWS metadata)
            if o[0] == 169 && o[1] == 254 {
                return true;
            }
            // 172.16.0.0/12
            if o[0] == 172 && (16..=31).contains(&o[1]) {
                return true;
            }
            // 192.168.0.0/16
            if o[0] == 192 && o[1] == 168 {
                return true;
            }
            // 224.0.0.0/4 multicast, 240.0.0.0/4 reserved
            if o[0] >= 224 {
                return true;
            }
            false
        }
        IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_unicast_link_local() || v6.is_unique_local() {
                return true;
            }
            // IPv4-mapped IPv6: ::ffff:a.b.c.d → segs[0..5]=0, segs[5]=0xffff
            let segs = v6.segments();
            if segs[0] == 0
                && segs[1] == 0
                && segs[2] == 0
                && segs[3] == 0
                && segs[4] == 0
                && segs[5] == 0xffff
            {
                let a = (segs[6] >> 8) as u8;
                let b = (segs[6] & 0xff) as u8;
                let c = (segs[7] >> 8) as u8;
                let d = (segs[7] & 0xff) as u8;
                return is_private_or_loopback(IpAddr::V4(Ipv4Addr::new(a, b, c, d)));
            }
            false
        }
    }
}

/// Verify a plugin tool TOML's Ed25519 signature.
///
/// Trust model (routing-debt.md Step 5): signatures are optional by
/// default — unsigned artifacts are allowed because everything in the
/// loom dir is user-owned local state. When `require_signature` is
/// true (config `[security]`), unsigned manifests are rejected.
///
/// Returns `Ok(())` if `public_key` and `signature` are present and the
/// signature authenticates the content, or if both are absent and
/// `require_signature` is false. Returns an error if one field is
/// present without the other, if the signature is invalid, or if both
/// are absent while `require_signature` is true.
pub fn verify_plugin_signature(
    manifest: &PluginManifest,
    content: &[u8],
    require_signature: bool,
) -> Result<()> {
    match (&manifest.public_key, &manifest.signature) {
        (Some(pk_b64), Some(sig_hex)) => {
            use ed25519_dalek::{Signature, Verifier, VerifyingKey};

            use base64::Engine;
            let pk_bytes = base64::engine::general_purpose::STANDARD
                .decode(pk_b64)
                .context("invalid base64 public_key")?;
            let pk =
                VerifyingKey::from_bytes(&pk_bytes.try_into().map_err(|_| {
                    anyhow::anyhow!("invalid public key length (expected 32 bytes)")
                })?)?;

            let sig_bytes = hex::decode(sig_hex).context("invalid hex signature")?;
            let sig = Signature::from_slice(&sig_bytes)
                .map_err(|e| anyhow::anyhow!("invalid signature: {e}"))?;

            pk.verify(content, &sig)
                .map_err(|e| anyhow::anyhow!("plugin signature verification failed: {e}"))?;

            Ok(())
        }
        (None, None) => {
            if require_signature {
                anyhow::bail!(
                    "plugin '{}' is unsigned and security.require_signature is enabled",
                    manifest.name
                );
            }
            Ok(()) // unsigned — allowed by default
        }
        (Some(_), None) => anyhow::bail!("plugin has public_key but no signature"),
        (None, Some(_)) => anyhow::bail!("plugin has signature but no public_key"),
    }
}

/// Path to the key-continuity pin store (`{loom_dir}/auth/plugin-pins.json`).
pub fn pins_path(loom_dir: &str) -> PathBuf {
    Path::new(loom_dir).join("auth").join("plugin-pins.json")
}

/// Read the pin store: tool_name → signing key (hex). Missing file → empty.
fn load_pins(loom_dir: &str) -> Result<HashMap<String, String>> {
    let path = pins_path(loom_dir);
    if !path.exists() {
        return Ok(HashMap::new());
    }
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&raw).with_context(|| format!("invalid pin store {}", path.display()))
}

/// Write the pin store (0600), reusing the auth-file permission pattern.
fn save_pins(loom_dir: &str, pins: &HashMap<String, String>) -> Result<()> {
    let path = pins_path(loom_dir);
    std::fs::create_dir_all(path.parent().unwrap())?;
    std::fs::write(&path, serde_json::to_string_pretty(pins)?)?;
    restrict_file_permissions(&path)
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(path, perms)?;
    Ok(())
}

#[cfg(not(unix))]
fn restrict_file_permissions(_path: &Path) -> Result<()> {
    Ok(())
}

/// Key-continuity pin check (TOFU with explicit acknowledgement): on the
/// first signed install of a tool name, record the signing key. On a later
/// install of the same name, refuse if the key differs — the user must
/// delete the pin entry to acknowledge the change.
pub fn check_and_write_pin(loom_dir: &str, tool_name: &str, public_key_b64: &str) -> Result<()> {
    use base64::Engine;
    let pk_bytes = base64::engine::general_purpose::STANDARD
        .decode(public_key_b64)
        .context("invalid base64 public_key")?;
    let key_hex = hex::encode(pk_bytes);

    let mut pins = load_pins(loom_dir)?;
    if let Some(pinned) = pins.get(tool_name) {
        if pinned != &key_hex {
            anyhow::bail!(
                "install refused: key changed since last install (pin mismatch) — delete {} entry for '{tool_name}' to acknowledge",
                pins_path(loom_dir).display()
            );
        }
    } else {
        pins.insert(tool_name.to_string(), key_hex);
        save_pins(loom_dir, &pins)?;
    }
    Ok(())
}

/// Produce the publisher-signed payload from a downloaded tool TOML:
/// the artifact with its own trust fields (`public_key` / `signature`
/// lines) removed — a signature cannot cover itself.
fn signed_payload(content: &str) -> String {
    content
        .lines()
        .filter(|l| {
            !(l.starts_with("public_key") || l.starts_with("signature "))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Shared trust gate for installs and updates: verify the manifest's
/// signature when present, enforce the `require_signature` posture
/// (default true — verify-by-default), and maintain key-continuity pins.
/// Nothing is written to disk before this passes.
fn enforce_trust(
    loom_dir: &Path,
    manifest: &PluginManifest,
    content: &str,
    require_signature: bool,
) -> Result<()> {
    let payload = signed_payload(content);
    let payload = payload.as_bytes();
    if manifest.public_key.is_some() && manifest.signature.is_some() {
        verify_plugin_signature(manifest, payload, true)?;
        let loom_dir_str = loom_dir.to_str().context("loom dir not valid UTF-8")?;
        check_and_write_pin(
            loom_dir_str,
            &manifest.name,
            manifest.public_key.as_deref().unwrap(),
        )?;
    } else if manifest.public_key.is_some() || manifest.signature.is_some() {
        // Half-signed manifests are always invalid.
        verify_plugin_signature(manifest, payload, false)?;
    } else if require_signature {
        anyhow::bail!(
            "install refused: plugin '{}' is unsigned — signature verification is on by default; set [security] require_signature = false to opt out",
            manifest.name
        );
    }
    Ok(())
}

/// Extract `public_key` / `signature` fields carried by the downloaded
/// tool TOML (publisher-signed artifacts embed them alongside the tool
/// config keys).
fn extract_trust_fields(parsed: &toml::Value) -> (Option<String>, Option<String>) {
    (
        parsed
            .get("public_key")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        parsed
            .get("signature")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    )
}

/// Manages the plugin registry stored in `~/.loom/plugins/`.
pub struct PluginRegistry {
    plugins_dir: PathBuf,
}

impl PluginRegistry {
    /// Create a new plugin registry rooted under `loom_dir/plugins`.
    pub fn new(loom_dir: &str) -> Self {
        let plugins_dir = Path::new(loom_dir).join("plugins");
        let _ = std::fs::create_dir_all(&plugins_dir);
        Self { plugins_dir }
    }

    /// Path to a plugin's TOML manifest.
    fn plugin_path(&self, name: &str) -> PathBuf {
        self.plugins_dir.join(format!("{name}.toml"))
    }

    /// List all registered plugins.
    pub fn list(&self) -> Result<Vec<PluginManifest>> {
        let mut plugins = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&self.plugins_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("toml") {
                    let raw = std::fs::read_to_string(&path)
                        .with_context(|| format!("Failed to read {}", path.display()))?;
                    match toml::from_str::<PluginManifest>(&raw) {
                        Ok(p) => plugins.push(p),
                        Err(e) => {
                            tracing::warn!("invalid plugin manifest '{}': {e}", path.display());
                        }
                    }
                }
            }
        }
        plugins.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(plugins)
    }

    /// Get a specific plugin by name.
    pub fn get(&self, name: &str) -> Result<PluginManifest> {
        let path = self.plugin_path(name);
        let raw = std::fs::read_to_string(&path)
            .with_context(|| format!("Plugin '{}' not found at {}", name, path.display()))?;
        let manifest: PluginManifest =
            toml::from_str(&raw).with_context(|| format!("Invalid plugin manifest '{}'", name))?;
        Ok(manifest)
    }

    /// Install a plugin from a tool TOML URL, creating a PluginManifest.
    ///
    /// Downloads are the only network step here; every trust decision
    /// (signature, pins) lives in [`Self::install_from_content`] so it can
    /// be tested without a server.
    pub fn install_from_url(
        &self,
        name: &str,
        url: &str,
        version: Option<&str>,
        require_signature: bool,
    ) -> Result<PluginManifest> {
        // Plugin downloads are HTTPS-only and SSRF-guarded (routing-debt.md Step 5)
        validate_outbound_url(url)?;

        // Attempt to download the tool TOML
        let response = ureq::get(url)
            .call()
            .map_err(|e| anyhow::anyhow!("Failed to download tool TOML from '{}': {}", url, e))?;

        let body = response
            .into_string()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;

        self.install_from_content(name, url, &body, version, require_signature)
    }

    /// Install a plugin from already-fetched tool TOML content.
    pub fn install_from_content(
        &self,
        name: &str,
        url: &str,
        content: &str,
        version: Option<&str>,
        require_signature: bool,
    ) -> Result<PluginManifest> {
        // Validate the plugin name (alphanumeric + hyphens)
        if !name
            .chars()
            .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
        {
            anyhow::bail!(
                "Invalid plugin name '{}': only alphanumeric, hyphens, and underscores allowed",
                name
            );
        }

        // Check if already installed
        if self.plugin_path(name).exists() {
            anyhow::bail!("Plugin '{}' is already installed", name);
        }

        // Validate that it's parseable TOML (must at least resemble a tool config)
        let parsed = toml::from_str::<toml::Value>(content)
            .map_err(|e| anyhow::anyhow!("Downloaded file is not valid TOML: {e}"))?;

        // Trust gate BEFORE anything is written (ADR 0005: verification is
        // the default path for remote installs).
        let (public_key, signature) = extract_trust_fields(&parsed);
        let manifest = PluginManifest {
            name: name.to_string(),
            version: version.unwrap_or("0.1.0").to_string(),
            description: parsed.get("description").and_then(|v| v.as_str()).map(str::to_string),
            author: None,
            homepage: None,
            repository: None,
            tool_toml_url: Some(url.to_string()),
            installed_at: Some(chrono::Utc::now().to_rfc3339()),
            checksum: None,
            public_key,
            signature,
        };

        let loom_dir = self.plugins_dir.parent().unwrap();
        enforce_trust(loom_dir, &manifest, content, require_signature)?;

        // Compute checksum after the trust gate
        let checksum = {
            let mut hasher = Sha256::new();
            hasher.update(content.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        // Save the tool TOML to the tools directory
        let tools_dir = loom_dir.join("tools");
        std::fs::create_dir_all(&tools_dir)?;
        let tool_path = tools_dir.join(format!("{name}.toml"));
        std::fs::write(&tool_path, content)?;

        // Record the linked tool
        println!("  Downloaded tool TOML to {}", tool_path.display());

        let manifest = PluginManifest { checksum: Some(checksum), ..manifest };

        self.save(&manifest)?;

        Ok(manifest)
    }

    /// Register a plugin from a local tool TOML file (no download).
    pub fn register_local(&self, name: &str, tool_path: &Path) -> Result<PluginManifest> {
        let raw = std::fs::read_to_string(tool_path)
            .with_context(|| format!("Failed to read {}", tool_path.display()))?;

        let checksum = {
            let mut hasher = Sha256::new();
            hasher.update(raw.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        // Copy into tools directory
        let tools_dir = self.plugins_dir.parent().unwrap().join("tools");
        std::fs::create_dir_all(&tools_dir)?;
        let dest = tools_dir.join(format!("{name}.toml"));
        std::fs::copy(tool_path, &dest)?;

        let manifest = PluginManifest {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            repository: None,
            tool_toml_url: None,
            installed_at: Some(chrono::Utc::now().to_rfc3339()),
            checksum: Some(checksum),
            public_key: None,
            signature: None,
        };

        self.save(&manifest)?;
        Ok(manifest)
    }

    /// Remove a plugin and its associated tool TOML.
    pub fn remove(&self, name: &str) -> Result<()> {
        // Remove plugin manifest
        let plugin_path = self.plugin_path(name);
        if plugin_path.exists() {
            std::fs::remove_file(&plugin_path)?;
        } else {
            anyhow::bail!("Plugin '{}' not found", name);
        }

        // Optionally remove the tool TOML (don't fail if it doesn't exist)
        let tools_dir = self.plugins_dir.parent().unwrap().join("tools");
        let tool_path = tools_dir.join(format!("{name}.toml"));
        if tool_path.exists() {
            std::fs::remove_file(&tool_path)?;
            println!("  Removed tool config: {}", tool_path.display());
        }

        Ok(())
    }

    /// Update a plugin: re-download tool TOML if it has a tool_toml_url.
    /// The new content passes the same trust gate as installs.
    pub fn update(&self, name: &str, require_signature: bool) -> Result<PluginManifest> {
        let existing = self.get(name)?;

        let url = existing.tool_toml_url.as_deref().ok_or_else(|| {
            anyhow::anyhow!("Plugin '{}' has no tool_toml_url to update from", name)
        })?;

        // Plugin downloads are HTTPS-only and SSRF-guarded (routing-debt.md Step 5)
        validate_outbound_url(url)?;

        // Download new version
        let response = ureq::get(url)
            .call()
            .map_err(|e| anyhow::anyhow!("Failed to download tool TOML: {}", e))?;

        let body = response
            .into_string()
            .map_err(|e| anyhow::anyhow!("Failed to read response body: {}", e))?;

        // Validate TOML
        if let Err(e) = toml::from_str::<toml::Value>(&body) {
            anyhow::bail!("Downloaded file is not valid TOML: {e}");
        }

        // Trust gate on the fresh content (ADR 0005), before overwrite.
        let parsed: toml::Value = toml::from_str(&body).unwrap();
        let (public_key, signature) = extract_trust_fields(&parsed);
        let mut fresh = existing.clone();
        fresh.public_key = public_key;
        fresh.signature = signature;
        enforce_trust(self.plugins_dir.parent().unwrap(), &fresh, &body, require_signature)?;

        // Compute new checksum
        let checksum = {
            let mut hasher = Sha256::new();
            hasher.update(body.as_bytes());
            format!("{:x}", hasher.finalize())
        };

        // Save tool TOML
        let tools_dir = self.plugins_dir.parent().unwrap().join("tools");
        let tool_path = tools_dir.join(format!("{name}.toml"));
        std::fs::write(&tool_path, &body)?;

        // Create updated manifest
        let mut updated = existing.clone();
        updated.checksum = Some(checksum);
        updated.installed_at = Some(chrono::Utc::now().to_rfc3339());
        // Bump minor version
        if let Ok(ver) = semver::Version::parse(&updated.version) {
            updated.version = format!("{}.{}.{}", ver.major, ver.minor + 1, ver.patch);
        }

        self.save(&updated)?;
        Ok(updated)
    }

    /// Search installed plugins by name/description.
    pub fn search(&self, query: &str) -> Result<Vec<PluginManifest>> {
        let query_lower = query.to_lowercase();
        let plugins = self.list()?;
        Ok(plugins
            .into_iter()
            .filter(|p| {
                p.name.to_lowercase().contains(&query_lower)
                    || p.description
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query_lower)
                    || p.author
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query_lower)
            })
            .collect())
    }

    /// Save a plugin manifest to disk.
    fn save(&self, manifest: &PluginManifest) -> Result<()> {
        std::fs::create_dir_all(&self.plugins_dir)?;
        let path = self.plugin_path(&manifest.name);
        let raw = toml::to_string_pretty(manifest)?;
        std::fs::write(&path, raw)?;
        Ok(())
    }

    /// Count installed plugins.
    pub fn count(&self) -> usize {
        self.list().map(|v| v.len()).unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_install_urls_must_be_https() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = PluginRegistry::new(tmp.path().to_str().unwrap());
        let err = registry
            .install_from_url("evil", "http://evil.example/x.toml", None, true)
            .unwrap_err();
        assert!(err.to_string().contains("HTTPS"), "got: {err}");
        // Rejected before any download: nothing was written.
        assert!(!tmp.path().join("plugins").join("evil.toml").exists());
    }

    #[test]
    fn plugin_update_urls_must_be_https() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = PluginRegistry::new(tmp.path().to_str().unwrap());
        // Hand-write a manifest pointing at an http:// URL.
        std::fs::create_dir_all(tmp.path().join("plugins")).unwrap();
        std::fs::write(
            tmp.path().join("plugins").join("x.toml"),
            "name = \"x\"\nversion = \"0.1.0\"\ntool_toml_url = \"http://evil.example/x.toml\"\n",
        )
        .unwrap();
        let err = registry.update("x", true).unwrap_err();
        assert!(err.to_string().contains("HTTPS"), "got: {err}");
    }

    fn unsigned_manifest() -> PluginManifest {
        PluginManifest {
            name: "x".to_string(),
            version: "0.1.0".to_string(),
            description: None,
            author: None,
            homepage: None,
            repository: None,
            tool_toml_url: None,
            installed_at: None,
            checksum: None,
            public_key: None,
            signature: None,
        }
    }

    #[test]
    fn unsigned_plugin_policy_follows_require_signature() {
        let m = unsigned_manifest();
        // Historic default: unsigned is allowed.
        assert!(verify_plugin_signature(&m, b"content", false).is_ok());
        // require_signature = true rejects unsigned manifests.
        let err = verify_plugin_signature(&m, b"content", true).unwrap_err();
        assert!(err.to_string().contains("unsigned"), "got: {err}");
    }

    #[test]
    fn half_signed_plugin_manifests_are_rejected() {
        let mut m = unsigned_manifest();
        m.public_key = Some("AAAA".to_string());
        assert!(verify_plugin_signature(&m, b"c", false).is_err());
        assert!(verify_plugin_signature(&m, b"c", true).is_err());

        let mut m = unsigned_manifest();
        m.signature = Some("deadbeef".to_string());
        assert!(verify_plugin_signature(&m, b"c", false).is_err());
        assert!(verify_plugin_signature(&m, b"c", true).is_err());
    }

    // ─── ADR 0005: verify-by-default + key-continuity pins ───────────

    /// Generate an Ed25519 keypair and return the manifest fields
    /// (public_key base64, signature hex) over `content`.
    fn signed_fields(
        content: &[u8],
    ) -> (String, String) {
        use base64::Engine;
        use ed25519_dalek::Signer;
        use rand_core::OsRng;

        let sk = ed25519_dalek::SigningKey::generate(&mut OsRng);
        let sig = sk.sign(content);
        (
            base64::engine::general_purpose::STANDARD.encode(sk.verifying_key().to_bytes()),
            hex::encode(sig.to_bytes()),
        )
    }

    fn content_text(name: &str) -> String {
        format!("[tool]\nname = \"{name}\"\ncommand = \"echo\"\n")
    }

    /// Strip trust lines the way `signed_payload` does, so tests sign
    /// exactly what the gate verifies.
    fn payload_of(content: &str) -> String {
        content
            .lines()
            .filter(|l| !(l.starts_with("public_key") || l.starts_with("signature ")))
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn valid_url() -> &'static str {
        "https://plugins.example/x.toml"
    }

    #[test]
    fn unsigned_install_refused_by_default_with_opt_out_named() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = PluginRegistry::new(tmp.path().to_str().unwrap());

        let err = registry
            .install_from_content(
                "x",
                valid_url(),
                &unsigned_content(),
                None,
                /* require_signature */ true,
            )
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains(
                "install refused: plugin 'x' is unsigned — signature verification is on by default; set [security] require_signature = false to opt out"
            ),
            "got: {msg}"
        );

        // Refusal is fail-closed: nothing was written.
        assert!(!tmp.path().join("plugins").join("x.toml").exists());
        assert!(!tmp.path().join("tools").join("x.toml").exists());

        // Explicit opt-out (require_signature = false in config) installs.
        let manifest = registry
            .install_from_content("x", valid_url(), &unsigned_content(), None, false)
            .unwrap();
        assert!(manifest.public_key.is_none());
    }

    fn unsigned_content() -> String {
        content_text("x")
    }

    #[test]
    fn signed_install_writes_pin_with_0600() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = PluginRegistry::new(tmp.path().to_str().unwrap());
        let content = content_text("x");
        let (public_key, signature) = signed_fields(payload_of(&content).as_bytes());
        let content_s = format!("public_key = \"{public_key}\"\nsignature = \"{signature}\"\n{content}");

        let manifest = registry
            .install_from_content("x", valid_url(), &content_s, None, true)
            .expect("valid signature must install");

        assert_eq!(manifest.public_key.as_deref(), Some(public_key.as_str()));

        // Pin recorded, hex of the raw key bytes.
        use base64::Engine;
        let pins_file = pins_path(tmp.path().to_str().unwrap());
        let pins_raw = std::fs::read_to_string(&pins_file).unwrap();
        let raw_key = base64::engine::general_purpose::STANDARD
            .decode(&public_key)
            .unwrap();
        assert!(pins_raw.contains(&hex::encode(raw_key)));

        // 0600 on unix (license.rs pattern).
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&pins_file)
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[test]
    fn pin_mismatch_refused_with_acknowledgement_path() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = PluginRegistry::new(tmp.path().to_str().unwrap());

        // Install under key A.
        let content_a = content_text("x");
        let (public_key_a, signature_a) = signed_fields(payload_of(&content_a).as_bytes());
        registry
            .install_from_content("x", valid_url(), &format!("public_key = \"{public_key_a}\"\nsignature = \"{signature_a}\"\n{content_a}"), None, true)
            .unwrap();

        // Uninstall, then re-install the same tool name under key B.
        std::fs::remove_file(tmp.path().join("plugins").join("x.toml")).unwrap();

        let content_b = content_text("x");
        let (public_key_b, signature_b) = signed_fields(payload_of(&content_b).as_bytes());
        assert_ne!(public_key_a, public_key_b);
        let err = registry
            .install_from_content("x", valid_url(), &format!("public_key = \"{public_key_b}\"\nsignature = \"{signature_b}\"\n{content_b}"), None, true)
            .unwrap_err();
        let pins = pins_path(tmp.path().to_str().unwrap());
        assert!(
            err.to_string().contains(
                "key changed since last install (pin mismatch) — delete",
            ),
            "got: {err}"
        );
        assert!(
            err.to_string().contains(&pins.display().to_string()),
            "must name the pin store path, got: {err}"
        );
    }

    #[test]
    fn unsigned_install_writes_no_pin() {
        let tmp = tempfile::tempdir().unwrap();
        let registry = PluginRegistry::new(tmp.path().to_str().unwrap());

        // Opted out: unsigned installs are allowed but must not record pins.
        registry
            .install_from_content("x", valid_url(), &unsigned_content(), None, false)
            .unwrap();
        assert!(!pins_path(tmp.path().to_str().unwrap()).exists());
    }

    #[test]
    fn validate_outbound_url_rejects_private_ips() {
        // IPv4 private / loopback / link-local / metadata
        for bad in [
            "https://127.0.0.1/",
            "https://127.0.0.1:8080/x",
            "https://10.0.0.5/",
            "https://172.16.0.1/",
            "https://172.31.255.255/",
            "https://192.168.1.1/",
            "https://169.254.169.254/latest/meta-data/",
            "https://0.0.0.0/",
            "https://224.0.0.1/",
        ] {
            let err = validate_outbound_url(bad).unwrap_err();
            assert!(
                err.to_string().contains("private/loopback"),
                "expected private/loopback error for {bad}, got: {err}"
            );
        }
        // IPv6 loopback / link-local / unique-local / IPv4-mapped
        for bad in [
            "https://[::1]/",
            "https://[::ffff:127.0.0.1]/",
            "https://[fe80::1]/",
            "https://[fc00::1]/",
        ] {
            let err = validate_outbound_url(bad).unwrap_err();
            assert!(
                err.to_string().contains("private/loopback"),
                "expected private/loopback error for {bad}, got: {err}"
            );
        }
        // Non-HTTPS still rejected
        let err = validate_outbound_url("http://127.0.0.1/").unwrap_err();
        assert!(err.to_string().contains("HTTPS"), "got: {err}");
        // Public HTTPS URLs are allowed (no download attempted — just validation)
        for good in [
            "https://example.com/x.toml",
            "https://example.com/",
            "https://www.example.org/path?query=1",
        ] {
            validate_outbound_url(good).expect("public HTTPS URL should pass: {good}");
        }
    }
}
