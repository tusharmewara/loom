//! `loom self update` — check for new versions and download updates.
//!
//! Uses the GitHub Releases API to discover the latest release, then downloads
//! the matching pre-built binary (Linux x86_64, macOS x86_64, macOS arm64) and
//! verifies it with a SHA-256 checksum before replacing the current binary.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;

const REPO_OWNER: &str = "tmewara";
const REPO_NAME: &str = "loom";
const GITHUB_API: &str = "https://api.github.com";
const USER_AGENT: &str = "loom-self-update/0.1";

/// Run the self-update workflow.
pub async fn run(force: bool) -> anyhow::Result<()> {
    let current_ver = env!("CARGO_PKG_VERSION");
    println!("Loom v{current_ver}");
    println!("Checking for updates...");

    let release = fetch_latest_release().await?;
    let latest_ver = release
        .tag_name
        .trim_start_matches('v');

    if !force && latest_ver == current_ver {
        println!("✓ Already up-to-date (v{current_ver})");
        return Ok(());
    }

    println!("  New version available: v{latest_ver} (current: v{current_ver})");

    let target = detect_target_triple()?;
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.contains(&target))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "No binary asset found for target '{target}' in release v{latest_ver}"
            )
        })?;

    println!("  Downloading {}...", asset.name);
    let bytes = download_binary(&asset.browser_download_url).await?;

    // Verify SHA-256 checksum if the release provides one
    if !release.checksum.is_empty() {
        let expected_checksum = &release.checksum;
        let actual_checksum = sha256_hex(&bytes);
        if &actual_checksum != expected_checksum {
            anyhow::bail!(
                "Checksum mismatch: expected {expected_checksum}, got {actual_checksum}"
            );
        }
        println!("  ✓ SHA-256 checksum verified");
    }

    // Find current binary path
    let current_exe = std::env::current_exe()
        .map_err(|e| anyhow::anyhow!("Cannot determine current executable path: {e}"))?;

    install_binary(&current_exe, &bytes, &target)?;

    println!("✓ Loom updated to v{latest_ver}");
    println!("  Restart loom to use the new version.");
    Ok(())
}

/// Fetch the latest release info from GitHub Releases API.
async fn fetch_latest_release() -> anyhow::Result<Release> {
    let url = format!("{GITHUB_API}/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let resp = ureq::get(&url)
        .set("User-Agent", USER_AGENT)
        .set("Accept", "application/vnd.github.v3+json")
        .call()
        .map_err(|e| anyhow::anyhow!("Failed to fetch release info: {e}"))?;

    let body = resp
        .into_string()
        .map_err(|e| anyhow::anyhow!("Failed to read response: {e}"))?;

    let json: serde_json::Value =
        serde_json::from_str(&body).map_err(|e| anyhow::anyhow!("Invalid JSON: {e}"))?;

    let tag_name = json["tag_name"]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("Missing tag_name in release response"))?
        .to_string();

    let assets = json["assets"]
        .as_array()
        .ok_or_else(|| anyhow::anyhow!("Missing assets in release response"))?;

    let mut checksum = String::new();
    for a in assets {
        if a["name"].as_str().map_or(false, |n| n.contains("sha256") || n.ends_with(".sha256")) {
            let url = a["browser_download_url"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Missing browser_download_url"))?;
            let resp = ureq::get(url)
                .set("User-Agent", USER_AGENT)
                .call()
                .map_err(|e| anyhow::anyhow!("Failed to fetch checksum: {e}"))?;
            let text = resp
                .into_string()
                .map_err(|e| anyhow::anyhow!("Failed to read checksum: {e}"))?;
            // Format: "<sha256>  <filename>"
            if let Some(hex) = text.split_whitespace().next() {
                checksum = hex.to_string();
            }
            break;
        }
    }

    let parsed_assets: Vec<ReleaseAsset> = assets
        .iter()
        .filter_map(|a| {
            Some(ReleaseAsset {
                name: a["name"].as_str()?.to_string(),
                browser_download_url: a["browser_download_url"].as_str()?.to_string(),
            })
        })
        .collect();

    Ok(Release {
        tag_name,
        assets: parsed_assets,
        checksum,
    })
}

/// Detect the platform target triple string used in release asset filenames.
fn detect_target_triple() -> anyhow::Result<String> {
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    match (os, arch) {
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu".to_string()),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu".to_string()),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin".to_string()),
        ("macos", "aarch64") => Ok("aarch64-apple-darwin".to_string()),
        _ => anyhow::bail!("Unsupported platform: {os}-{arch}"),
    }
}

/// Download a binary from a URL into memory.
async fn download_binary(url: &str) -> anyhow::Result<Vec<u8>> {
    let resp = ureq::get(url)
        .set("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| anyhow::anyhow!("Download failed: {e}"))?;

    let len = resp
        .header("Content-Length")
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(0);

    let mut bytes = Vec::with_capacity(len);
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| anyhow::anyhow!("Failed to read response body: {e}"))?;

    Ok(bytes)
}

/// Compute SHA-256 hex of bytes.
fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// Replace the current executable with the downloaded bytes.
///
/// On Unix, writes to a temp file alongside the target, then atomically
/// renames it over the original.
fn install_binary(current_exe: &PathBuf, bytes: &[u8], target: &str) -> anyhow::Result<()> {
    let parent = current_exe
        .parent()
        .ok_or_else(|| anyhow::anyhow!("Cannot determine parent directory of executable"))?;

    let tmp_path = parent.join(format!(".loom-update-{target}.tmp"));
    std::fs::write(&tmp_path, bytes)?;

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&tmp_path, std::fs::Permissions::from_mode(0o755))?;
    }

    // Atomically replace the original
    std::fs::rename(&tmp_path, current_exe)?;

    Ok(())
}

// ─── Data types ───────────────────────────────────────────────────────

struct Release {
    tag_name: String,
    assets: Vec<ReleaseAsset>,
    checksum: String,
}

struct ReleaseAsset {
    name: String,
    browser_download_url: String,
}
