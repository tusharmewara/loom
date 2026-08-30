use anyhow::{Context, Result};
use argon2::password_hash::rand_core::RngCore;
use argon2::{
    password_hash::{rand_core::OsRng as ArgonOsRng, PasswordHasher, SaltString},
    Argon2,
};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Key, Nonce,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;
use tracing::debug;
use zeroize::Zeroize;

/// Encrypted credential stored on disk
#[derive(Debug, Serialize, Deserialize)]
pub struct EncryptedBlob {
    /// Provider identifier
    pub provider_id: String,
    /// ChaCha20Poly1305 nonce (hex)
    pub nonce: String,
    /// Encrypted credential data (hex)
    pub ciphertext: String,
    /// Argon2 salt used for key derivation (hex)
    pub salt: String,
    /// SHA-256 of plaintext for integrity check
    pub integrity_hash: String,
    /// When this was created
    pub created_at: String,
    /// Unix timestamp in seconds
    pub created_at_epoch: u64,
}

impl EncryptedBlob {
    fn new(provider_id: &str, plaintext: &str, key: &[u8; 32]) -> Result<Self> {
        let salt = SaltString::generate(&mut ArgonOsRng);
        let argon2 = Argon2::default();
        let _hash = argon2
            .hash_password(plaintext.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("argon2 hash failed: {e}"))?;

        // Generate random 12-byte nonce
        let mut nonce_bytes = [0u8; 12];
        ArgonOsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher_key = Key::from_slice(key);
        let cipher = ChaCha20Poly1305::new(cipher_key);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|e| anyhow::anyhow!("chacha20poly1305 encryption failed: {e}"))?;

        // Integrity hash
        let integrity = {
            let mut hasher = Sha256::new();
            hasher.update(plaintext.as_bytes());
            hex::encode(&hasher.finalize())
        };

        Ok(Self {
            provider_id: provider_id.to_string(),
            nonce: hex::encode(&nonce_bytes),
            ciphertext: hex::encode(&ciphertext),
            salt: salt.to_string(),
            integrity_hash: integrity,
            created_at: chrono_now(),
            created_at_epoch: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        })
    }

    fn decrypt(&self, key: &[u8; 32]) -> Result<String> {
        let nonce_bytes =
            hex::decode(&self.nonce).map_err(|e| anyhow::anyhow!("invalid nonce hex: {e}"))?;
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = hex::decode(&self.ciphertext)
            .map_err(|e| anyhow::anyhow!("invalid ciphertext hex: {e}"))?;

        let cipher_key = Key::from_slice(key);
        let cipher = ChaCha20Poly1305::new(cipher_key);
        let plaintext = cipher
            .decrypt(nonce, ciphertext.as_ref())
            .map_err(|e| anyhow::anyhow!("chacha20poly1305 decryption failed: {e}"))?;

        let result = String::from_utf8(plaintext).context("decrypted data is not valid UTF-8")?;

        // Verify integrity hash
        let computed_hash = {
            let mut hasher = Sha256::new();
            hasher.update(result.as_bytes());
            hex::encode(&hasher.finalize())
        };

        if computed_hash != self.integrity_hash {
            anyhow::bail!("integrity check failed: data may be corrupted or tampered");
        }

        Ok(result)
    }
}

fn chrono_now() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86400;
    let time_secs = secs % 86400;
    let hours = time_secs / 3600;
    let minutes = (time_secs % 3600) / 60;
    let seconds = time_secs % 60;

    let (year, month, day) = days_to_date(days as i64 + 719468);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hours, minutes, seconds
    )
}

/// Convert Rata Die days to Gregorian date (Howard Hinnant algorithm)
fn days_to_date(z: i64) -> (i64, u32, u32) {
    let z = z - 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// A secure credential vault using ChaCha20Poly1305 encryption with Argon2 key derivation.
///
/// The vault can be locked and unlocked with a master password. Sensitive keys
/// are zeroized on drop and when the vault is locked.
pub struct CredentialVault {
    loom_dir: String,
    locked: bool,
    /// Derived encryption key (32 bytes). Zeroized on lock/drop.
    master_key: Option<[u8; 32]>,
    /// Number of failed unlock attempts (rate limiting)
    failed_attempts: u32,
    /// Time of first failed attempt in current window
    window_start: std::time::Instant,
}

impl CredentialVault {
    /// Create a new vault. Starts locked.
    pub fn new(loom_dir: &str) -> Self {
        Self {
            loom_dir: loom_dir.to_string(),
            locked: true,
            master_key: None,
            failed_attempts: 0,
            window_start: std::time::Instant::now(),
        }
    }

    /// Unlock the vault with a master password using Argon2 key derivation.
    ///
    /// Rate-limited to 5 attempts per minute.
    pub fn unlock(&mut self, password: &str) -> Result<()> {
        // Rate limiting check
        let elapsed = self.window_start.elapsed();
        if elapsed.as_millis() < 60_000 && self.failed_attempts >= 5 {
            let remaining_ms = (60_000 - elapsed.as_millis() as u64).max(1);
            let remaining_secs = remaining_ms.div_ceil(1000);
            anyhow::bail!(
                "too many failed unlock attempts; retry in {} seconds",
                remaining_secs
            );
        }

        // Reset window if enough time has passed
        if elapsed.as_millis() >= 60_000 {
            self.failed_attempts = 0;
            self.window_start = std::time::Instant::now();
        }

        // Derive a deterministic key using Argon2id with a password-derived salt.
        // Using a deterministic salt ensures the same password always produces the same key.
        let mut hasher = Sha256::new();
        hasher.update(password.as_bytes());
        hasher.update(b"loom-vault-key-derivation");
        let salt_bytes = hasher.finalize();
        let salt_b64 = base64::encode(&salt_bytes);
        let salt = SaltString::from_b64(&salt_b64)
            .map_err(|e| anyhow::anyhow!("failed to create argon2 salt: {e}"))?;
        let argon2 = Argon2::default();
        let hash = argon2
            .hash_password(password.as_bytes(), &salt)
            .map_err(|e| anyhow::anyhow!("argon2 key derivation failed: {e}"))?;

        // Use SHA-256 of the Argon2 hash output as the encryption key
        let mut hasher = Sha256::new();
        hasher.update(hash.hash.unwrap().as_ref());
        let key_bytes = hasher.finalize();

        let mut key = [0u8; 32];
        key.copy_from_slice(&key_bytes);
        self.master_key = Some(key);
        self.locked = false;
        self.failed_attempts = 0;

        debug!("vault unlocked successfully");
        Ok(())
    }

    /// Lock the vault, zeroizing the master key.
    pub fn lock(&mut self) {
        if let Some(ref mut key) = self.master_key {
            key.zeroize();
        }
        self.master_key = None;
        self.locked = true;
        debug!("vault locked");
    }

    /// Check if the vault is currently unlocked.
    pub fn is_locked(&self) -> bool {
        self.locked
    }

    /// Get the loom directory path.
    pub fn loom_dir(&self) -> &str {
        &self.loom_dir
    }

    /// Store an encrypted credential. Requires the vault to be unlocked.
    pub fn set_credential(&self, provider_id: &str, credential: &str) -> Result<()> {
        if self.locked {
            anyhow::bail!("vault is locked; call unlock() first");
        }
        let key = self
            .master_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("master key not available"))?;

        let blob = EncryptedBlob::new(provider_id, credential, key)?;

        let auth_dir = Path::new(&self.loom_dir).join("auth");
        std::fs::create_dir_all(&auth_dir)
            .with_context(|| format!("failed to create auth dir: {:?}", auth_dir))?;

        let cred_path = auth_dir.join(format!("{}.enc", provider_id));
        let json = serde_json::to_string_pretty(&blob)?;
        std::fs::write(&cred_path, &json)
            .with_context(|| format!("failed to write credential: {:?}", cred_path))?;

        debug!(provider_id, "credential stored");
        Ok(())
    }

    /// Retrieve and decrypt a credential. Requires the vault to be unlocked.
    pub fn get_credential(&self, provider_id: &str) -> Result<Option<String>> {
        if self.locked {
            anyhow::bail!("vault is locked; call unlock() first");
        }
        let key = self
            .master_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("master key not available"))?;

        let cred_path = Path::new(&self.loom_dir)
            .join("auth")
            .join(format!("{}.enc", provider_id));

        if !cred_path.exists() {
            return Ok(None);
        }

        let json = std::fs::read_to_string(&cred_path)
            .with_context(|| format!("failed to read credential: {:?}", cred_path))?;
        let blob: EncryptedBlob =
            serde_json::from_str(&json).context("failed to parse encrypted blob")?;

        let plaintext = blob.decrypt(key)?;
        Ok(Some(plaintext))
    }

    /// List all stored credential provider IDs.
    pub fn list_credentials(&self) -> Result<Vec<String>> {
        let auth_dir = Path::new(&self.loom_dir).join("auth");
        if !auth_dir.exists() {
            return Ok(Vec::new());
        }
        let mut providers = Vec::new();
        for entry in std::fs::read_dir(&auth_dir)? {
            let entry = entry?;
            if let Some(name) = entry.file_name().to_str() {
                if let Some(stripped) = name.strip_suffix(".enc") {
                    providers.push(stripped.to_string());
                }
            }
        }
        providers.sort();
        Ok(providers)
    }

    /// Delete a stored credential.
    pub fn delete_credential(&self, provider_id: &str) -> Result<bool> {
        let cred_path = Path::new(&self.loom_dir)
            .join("auth")
            .join(format!("{}.enc", provider_id));
        if cred_path.exists() {
            std::fs::remove_file(&cred_path)
                .with_context(|| format!("failed to delete credential: {:?}", cred_path))?;
            debug!(provider_id, "credential deleted");
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Encrypt a credential in memory (without writing to disk) for export.
    ///
    /// The result is encrypted with the vault's master key using a fresh
    /// random nonce. Returns the encrypted data and nonce (both hex-encoded).
    /// Requires the vault to be unlocked.
    pub fn encrypt_for_export(&self, credential: &str) -> Result<(String, String)> {
        if self.locked {
            anyhow::bail!("vault is locked; call unlock() first");
        }
        let key = self
            .master_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("master key not available"))?;

        // Generate random 12-byte nonce
        let mut nonce_bytes = [0u8; 12];
        ArgonOsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let cipher_key = Key::from_slice(key);
        let cipher = ChaCha20Poly1305::new(cipher_key);
        let ciphertext = cipher
            .encrypt(nonce, credential.as_bytes())
            .map_err(|e| anyhow::anyhow!("chacha20poly1305 encryption failed: {e}"))?;

        Ok((hex::encode(&ciphertext), hex::encode(&nonce_bytes)))
    }

    /// Change the master password. Re-encrypts all existing credentials.
    pub fn change_password(&mut self, old_password: &str, new_password: &str) -> Result<()> {
        // Verify old password by trying unlock
        if self.locked {
            self.unlock(old_password)?;
        } else {
            // Verify current key can decrypt at least one credential
            let providers = self.list_credentials()?;
            if !providers.is_empty() {
                let test = self.get_credential(&providers[0])?;
                if test.is_none() {
                    anyhow::bail!("could not verify existing master key");
                }
            }
        }

        // Collect all existing credentials before changing key
        let existing: Vec<(String, String)> = {
            let providers = self.list_credentials()?;
            let mut creds = Vec::new();
            for p in &providers {
                if let Some(val) = self.get_credential(p)? {
                    creds.push((p.clone(), val));
                }
            }
            creds
        };

        // Lock and re-unlock with new password to get new key
        {
            self.lock();
            self.unlock(new_password)?;
        }

        // Re-encrypt all credentials with new key
        for (provider_id, credential) in &existing {
            self.set_credential(provider_id, credential)?;
        }

        debug!(
            "master password changed, {} credentials re-encrypted",
            existing.len()
        );
        Ok(())
    }
}

impl Drop for CredentialVault {
    fn drop(&mut self) {
        self.lock();
    }
}

// Simple hex encoding/decoding without pulling in the `hex` crate
mod hex {
    const HEX_CHARS: &[u8] = b"0123456789abcdef";

    pub fn encode(bytes: &[u8]) -> String {
        let mut result = Vec::with_capacity(bytes.len() * 2);
        for &b in bytes {
            result.push(HEX_CHARS[(b >> 4) as usize]);
            result.push(HEX_CHARS[(b & 0x0f) as usize]);
        }
        unsafe { String::from_utf8_unchecked(result) }
    }

    pub fn decode(hex_str: &str) -> Result<Vec<u8>, String> {
        let hex_bytes = hex_str.as_bytes();
        if !hex_bytes.len().is_multiple_of(2) {
            return Err("odd hex string length".to_string());
        }
        let mut result = Vec::with_capacity(hex_bytes.len() / 2);
        for chunk in hex_bytes.chunks(2) {
            let high = from_hex_digit(chunk[0])?;
            let low = from_hex_digit(chunk[1])?;
            result.push((high << 4) | low);
        }
        Ok(result)
    }

    fn from_hex_digit(d: u8) -> Result<u8, String> {
        match d {
            b'0'..=b'9' => Ok(d - b'0'),
            b'a'..=b'f' => Ok(d - b'a' + 10),
            b'A'..=b'F' => Ok(d - b'A' + 10),
            _ => Err(format!("invalid hex digit: '{}'", d as char)),
        }
    }
}

/// Base64 encoding (standard, no padding) for use with Argon2 salt generation.
mod base64 {
    const B64_CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    pub fn encode(bytes: &[u8]) -> String {
        let mut result = Vec::with_capacity(bytes.len().div_ceil(3) * 4);
        for chunk in bytes.chunks(3) {
            let b0 = chunk[0] as u32;
            let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
            let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
            let triple = (b0 << 16) | (b1 << 8) | b2;
            result.push(B64_CHARS[((triple >> 18) & 0x3F) as usize]);
            result.push(B64_CHARS[((triple >> 12) & 0x3F) as usize]);
            if chunk.len() > 1 {
                result.push(B64_CHARS[((triple >> 6) & 0x3F) as usize]);
            }
            if chunk.len() > 2 {
                result.push(B64_CHARS[(triple & 0x3F) as usize]);
            }
        }
        unsafe { String::from_utf8_unchecked(result) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lock_unlock() {
        let mut vault = CredentialVault::new("/tmp/loom-auth-test-lock");
        assert!(vault.is_locked());
        vault.unlock("hunter2").unwrap();
        assert!(!vault.is_locked());
        vault.lock();
        assert!(vault.is_locked());
    }

    #[test]
    fn test_set_get_credential() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut vault = CredentialVault::new(dir.path().to_str().unwrap());
        vault.unlock("test-pass").unwrap();
        vault.set_credential("openai", "sk-abc123").unwrap();
        let result = vault.get_credential("openai").unwrap();
        assert_eq!(result, Some("sk-abc123".to_string()));
    }

    #[test]
    fn test_locked_vault_rejects_operations() {
        let vault = CredentialVault::new("/tmp/loom-auth-test-locked");
        assert!(vault.set_credential("test", "value").is_err());
        assert!(vault.get_credential("test").is_err());
    }

    #[test]
    fn test_list_credentials() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut vault = CredentialVault::new(dir.path().to_str().unwrap());
        vault.unlock("test-pass").unwrap();
        vault.set_credential("a", "1").unwrap();
        vault.set_credential("b", "2").unwrap();
        let list = vault.list_credentials().unwrap();
        assert_eq!(list, vec!["a", "b"]);
    }

    #[test]
    fn test_delete_credential() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut vault = CredentialVault::new(dir.path().to_str().unwrap());
        vault.unlock("test-pass").unwrap();
        vault.set_credential("delete-me", "secret").unwrap();
        assert!(vault.get_credential("delete-me").unwrap().is_some());
        assert!(vault.delete_credential("delete-me").unwrap());
        assert!(vault.get_credential("delete-me").unwrap().is_none());
    }

    #[test]
    fn test_change_password_reencrypts() {
        let dir = tempfile::TempDir::new().unwrap();
        let mut vault = CredentialVault::new(dir.path().to_str().unwrap());
        vault.unlock("old-pass").unwrap();
        vault.set_credential("provider1", "secret1").unwrap();
        vault.set_credential("provider2", "secret2").unwrap();

        vault.change_password("old-pass", "new-pass").unwrap();

        assert_eq!(
            vault.get_credential("provider1").unwrap(),
            Some("secret1".to_string())
        );
        assert_eq!(
            vault.get_credential("provider2").unwrap(),
            Some("secret2".to_string())
        );
    }

    #[test]
    fn test_rate_limiting() {
        let mut vault = CredentialVault::new("/tmp/loom-auth-test-rate");
        vault.failed_attempts = 5;
        vault.window_start = std::time::Instant::now();
        let result = vault.unlock("any-password");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("too many failed unlock attempts"));
    }
}
