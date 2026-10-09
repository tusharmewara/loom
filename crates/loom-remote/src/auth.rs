//! SSH authentication key loading.

/// Source of an SSH private key.
#[derive(Debug, Clone)]
pub enum SshKeySource {
    /// Path to a private key file on disk.
    File(String),
    /// Raw PEM-encoded key material (from vault).
    Pem(String),
    /// Use ssh-agent (default).
    Agent,
}

/// Load an SSH private key from the given source.
/// Returns the key bytes (PEM format) or an error.
pub fn load_ssh_key(source: &SshKeySource) -> anyhow::Result<String> {
    match source {
        SshKeySource::File(path) => {
            let expanded = shellexpand::tilde(path);
            let key = std::fs::read_to_string(expanded.as_ref())
                .map_err(|e| anyhow::anyhow!("Failed to read SSH key '{}': {}", path, e))?;
            Ok(key)
        }
        SshKeySource::Pem(pem) => Ok(pem.clone()),
        SshKeySource::Agent => anyhow::bail!("Use ssh-agent directly via the SSH connection manager"),
    }
}

