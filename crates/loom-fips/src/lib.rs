//! `loom-fips` — FIPS-compliant crypto and air-gap support.

use anyhow::{Context, Result};
use ring::aead::{AES_256_GCM, LessSafeKey, Nonce, UnboundKey};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{Ed25519KeyPair, KeyPair, ED25519};
use zeroize::Zeroize;

/// Returns true if FIPS mode is enabled via `LOOM_FIPS=1` env var.
pub fn fips_enabled() -> bool {
    std::env::var("LOOM_FIPS").is_ok()
}

/// Returns true if air-gap mode is enabled via `LOOM_AIR_GAP=1` env var.
pub fn air_gap_enabled() -> bool {
    std::env::var("LOOM_AIR_GAP").is_ok()
}

/// Generate Ed25519 keypair. Uses `ring` in FIPS mode, `ed25519-dalek` otherwise.
pub fn ed25519_keypair() -> Result<(Vec<u8>, Vec<u8>)> {
    if fips_enabled() {
        ed25519_keypair_fips()
    } else {
        #[cfg(feature = "ed25519-dalek")]
        {
            ed25519_keypair_dalek()
        }
        #[cfg(not(feature = "ed25519-dalek"))]
        {
            ed25519_keypair_fips()
        }
    }
}

fn ed25519_keypair_fips() -> Result<(Vec<u8>, Vec<u8>)> {
    let rng = SystemRandom::new();
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ED25519, &rng)
        .context("failed to generate Ed25519 keypair")?;
    let keypair = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .context("failed to parse keypair")?;
    let private_key = keypair.private_key().to_vec();
    let public_key = keypair.public_key().as_ref().to_vec();
    Ok((private_key, public_key))
}

#[cfg(feature = "ed25519-dalek")]
fn ed25519_keypair_dalek() -> Result<(Vec<u8>, Vec<u8>)> {
    use ed25519_dalek::{Keypair, Signer};
    use rand::rngs::OsRng;
    let mut csprng = OsRng;
    let keypair: Keypair = Keypair::generate(&mut csprng);
    let private_key = keypair.secret.to_bytes().to_vec();
    let public_key = keypair.public.to_bytes().to_vec();
    Ok((private_key, public_key))
}

/// Sign message with Ed25519 private key.
pub fn ed25519_sign(private_key: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    if fips_enabled() {
        ed25519_sign_fips(private_key, message)
    } else {
        #[cfg(feature = "ed25519-dalek")]
        {
            ed25519_sign_dalek(private_key, message)
        }
        #[cfg(not(feature = "ed25519-dalek"))]
        {
            ed25519_sign_fips(private_key, message)
        }
    }
}

fn ed25519_sign_fips(private_key: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    let keypair = Ed25519KeyPair::from_pkcs8(private_key)
        .context("invalid private key")?;
    let sig = keypair.sign(message);
    Ok(sig.as_ref().to_vec())
}

#[cfg(feature = "ed25519-dalek")]
fn ed25519_sign_dalek(private_key: &[u8], message: &[u8]) -> Result<Vec<u8>> {
    use ed25519_dalek::{SecretKey, Signer};
    let secret = SecretKey::from_bytes(private_key.try_into().context("invalid key length")?)?;
    let sig = secret.sign(message);
    Ok(sig.to_bytes().to_vec())
}

/// Verify Ed25519 signature.
pub fn ed25519_verify(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    if fips_enabled() {
        ed25519_verify_fips(public_key, message, signature)
    } else {
        #[cfg(feature = "ed25519-dalek")]
        {
            ed25519_verify_dalek(public_key, message, signature)
        }
        #[cfg(not(feature = "ed25519-dalek"))]
        {
            ed25519_verify_fips(public_key, message, signature)
        }
    }
}

fn ed25519_verify_fips(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    use ring::signature::UnparsedPublicKey;
    let peer_key = UnparsedPublicKey::new(&ED25519, public_key);
    peer_key.verify(message, signature).is_ok()
}

#[cfg(feature = "ed25519-dalek")]
fn ed25519_verify_dalek(public_key: &[u8], message: &[u8], signature: &[u8]) -> bool {
    use ed25519_dalek::{PublicKey, Signature, Verifier};
    let Ok(pub_key) = PublicKey::from_bytes(public_key.try_into().ok()?) else { return false; };
    let Ok(sig) = Signature::from_bytes(signature.try_into().ok()?) else { return false; };
    pub_key.verify(message, &sig).is_ok()
}

/// Encrypt config with AES-256-GCM (FIPS-approved).
/// Returns base64(nonce || ciphertext).
pub fn encrypt_config(key: &[u8; 32], plaintext: &[u8]) -> Result<String> {
    let unbound = UnboundKey::new(&AES_256_GCM, key).context("invalid key")?;
    let key = LessSafeKey::new(unbound);
    let rng = SystemRandom::new();
    let mut nonce_bytes = [0u8; 12];
    rng.fill(&mut nonce_bytes).context("RNG failed")?;
    let nonce = Nonce::assume_unique_for_key(nonce_bytes);
    let mut ciphertext = plaintext.to_vec();
    key.seal_in_place_append_tag(nonce, ring::aead::Aad::empty(), &mut ciphertext)
        .context("encryption failed")?;
    let mut out = nonce_bytes.to_vec();
    out.extend_from_slice(&ciphertext);
    Ok(base64::encode(&out))
}

/// Decrypt config with AES-256-GCM.
pub fn decrypt_config(key: &[u8; 32], ciphertext_b64: &str) -> Result<Vec<u8>> {
    let data = base64::decode(ciphertext_b64).context("invalid base64")?;
    if data.len() < 12 {
        return Err(anyhow::anyhow!("ciphertext too short"));
    }
    let (nonce_bytes, ciphertext) = data.split_at(12);
    let mut nonce = [0u8; 12];
    nonce.copy_from_slice(nonce_bytes);
    let unbound = UnboundKey::new(&AES_256_GCM, key).context("invalid key")?;
    let key = LessSafeKey::new(unbound);
    let mut plaintext = ciphertext.to_vec();
    key.open_in_place(Nonce::assume_unique_for_key(nonce), ring::aead::Aad::empty(), &mut plaintext)
        .context("decryption failed")?;
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fips_flag_works() {
        std::env::set_var("LOOM_FIPS", "1");
        assert!(fips_enabled());
        std::env::remove_var("LOOM_FIPS");
        assert!(!fips_enabled());
    }

    #[test]
    fn air_gap_flag_works() {
        std::env::set_var("LOOM_AIR_GAP", "1");
        assert!(air_gap_enabled());
        std::env::remove_var("LOOM_AIR_GAP");
        assert!(!air_gap_enabled());
    }

    #[test]
    fn keypair_generation_works() {
        let (sk, pk) = ed25519_keypair().unwrap();
        assert_eq!(sk.len(), 32); // PKCS8 is longer, but we return raw 32 bytes in non-fips
        assert_eq!(pk.len(), 32);
    }

    #[test]
    fn sign_verify_roundtrip() {
        let (sk, pk) = ed25519_keypair().unwrap();
        let msg = b"test message";
        let sig = ed25519_sign(&sk, msg).unwrap();
        assert!(ed25519_verify(&pk, msg, &sig));
        assert!(!ed25519_verify(&pk, b"different", &sig));
    }

    #[test]
    fn config_encrypt_decrypt_roundtrip() {
        let key = [42u8; 32];
        let plaintext = b"secret config data";
        let encrypted = encrypt_config(&key, plaintext).unwrap();
        let decrypted = decrypt_config(&key, &encrypted).unwrap();
        assert_eq!(plaintext, &decrypted[..]);
    }

    #[test]
    fn config_decrypt_wrong_key_fails() {
        let key1 = [1u8; 32];
        let key2 = [2u8; 32];
        let plaintext = b"secret";
        let encrypted = encrypt_config(&key1, plaintext).unwrap();
        assert!(decrypt_config(&key2, &encrypted).is_err());
    }
}