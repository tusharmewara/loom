use anyhow::{bail, Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use loom_core::HookEntry;
use sha2::{Digest, Sha256};

/// Canonical payload for signing (hook metadata without the signature field).
pub fn hook_signing_payload(entry: &HookEntry) -> Result<Vec<u8>> {
    let mut clone = entry.clone();
    clone.signature = None;
    Ok(serde_json::to_vec(&clone)?)
}

/// Sign a hook entry with an Ed25519 private key (hex-encoded 32 bytes).
pub fn sign_hook(entry: &mut HookEntry, private_key_hex: &str) -> Result<()> {
    let key_bytes = hex_decode(private_key_hex)?;
    let signing_key = signing_key_from_bytes(&key_bytes)?;
    if entry.public_key.is_none() {
        entry.public_key = Some(hex_encode(signing_key.verifying_key().to_bytes()));
    }
    let payload = hook_signing_payload(entry)?;
    let sig = signing_key.sign(&payload);
    entry.signature = Some(hex_encode(sig.to_bytes()));
    Ok(())
}

/// Verify a hook entry's signature against its public key.
pub fn verify_hook(entry: &HookEntry) -> Result<bool> {
    let Some(sig_hex) = &entry.signature else {
        return Ok(true);
    };
    let Some(pk_hex) = &entry.public_key else {
        bail!("hook '{}' has signature but no public_key", entry.id);
    };

    let verifying_key = verifying_key_from_hex(pk_hex)?;
    let signature = signature_from_hex(sig_hex)?;
    let payload = hook_signing_payload(entry)?;
    Ok(verifying_key.verify(&payload, &signature).is_ok())
}

pub fn generate_keypair_hex() -> (String, String) {
    let mut rng = rand::rngs::OsRng;
    let signing_key = SigningKey::generate(&mut rng);
    let verifying_key = signing_key.verifying_key();
    (
        hex_encode(signing_key.to_bytes()),
        hex_encode(verifying_key.to_bytes()),
    )
}

pub fn hash_script(content: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content);
    format!("{:x}", hasher.finalize())
}

fn hex_encode(bytes: impl AsRef<[u8]>) -> String {
    bytes
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn hex_decode(s: &str) -> Result<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        bail!("invalid hex length");
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).context("invalid hex"))
        .collect()
}

fn signing_key_from_bytes(bytes: &[u8]) -> Result<SigningKey> {
    match bytes.len() {
        32 => {
            let arr: [u8; 32] = bytes.try_into().unwrap();
            Ok(SigningKey::from_bytes(&arr))
        }
        64 => {
            let arr: [u8; 32] = bytes[..32].try_into().unwrap();
            Ok(SigningKey::from_bytes(&arr))
        }
        _ => bail!("private key must be 32 or 64 bytes (hex)"),
    }
}

fn verifying_key_from_hex(hex_str: &str) -> Result<VerifyingKey> {
    let bytes = hex_decode(hex_str)?;
    let arr: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("public key must be 32 bytes"))?;
    VerifyingKey::from_bytes(&arr).map_err(|e| anyhow::anyhow!("invalid public key: {e}"))
}

fn signature_from_hex(hex_str: &str) -> Result<Signature> {
    let bytes = hex_decode(hex_str)?;
    let arr: [u8; 64] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("signature must be 64 bytes"))?;
    Ok(Signature::from_bytes(&arr))
}
