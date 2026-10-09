//! ACP secure handshake — Ed25519-based challenge-response authentication.
//!
//! Provides:
//! - Key pair generation
//! - Challenge-response authentication
//! - Session key derivation (HMAC-SHA256)
//! - Peer identity verification

use anyhow::{Context, Result};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand_core::{OsRng, RngCore};
use sha2::{Digest, Sha256};
use std::fmt;

// ─── manual HMAC-SHA256 ──────────────────────────────────────────────
// Avoids the `digest` version conflict between sha2 0.10 (digest 0.10)
// and the `hmac` crate 0.13 (digest 0.11).

const BLOCK_SIZE: usize = 64; // SHA-256 block size

/// Compute HMAC-SHA256(key, data) per RFC 2104.
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    // Prepare the key: if longer than block size, hash it first
    let k = if key.len() > BLOCK_SIZE {
        let hash = Sha256::digest(key);
        let mut kp = [0u8; BLOCK_SIZE];
        kp[..32].copy_from_slice(&hash);
        kp
    } else {
        let mut kp = [0u8; BLOCK_SIZE];
        kp[..key.len()].copy_from_slice(key);
        kp
    };

    // ipad = key XOR 0x36, opad = key XOR 0x5c
    let mut ipad = [0x36u8; BLOCK_SIZE];
    let mut opad = [0x5cu8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    // inner = SHA256((key XOR ipad) || data)
    let mut inner_hasher = Sha256::new();
    inner_hasher.update(ipad);
    inner_hasher.update(data);
    let inner = inner_hasher.finalize();

    // outer = SHA256((key XOR opad) || inner)
    let mut outer_hasher = Sha256::new();
    outer_hasher.update(opad);
    outer_hasher.update(inner);

    outer_hasher.finalize().into()
}

// ─── Key pair ────────────────────────────────────────────────────────

/// An Ed25519 key pair for ACP authentication.
#[derive(Clone)]
pub struct AcpKeyPair {
    /// The signing key (private).
    signing_key: SigningKey,
    /// The verifying key (public).
    pub verifying_key: VerifyingKey,
}

impl AcpKeyPair {
    /// Generate a new Ed25519 key pair using the OS secure RNG.
    pub fn generate() -> Self {
        let mut csprng = OsRng;
        let signing_key = SigningKey::generate(&mut csprng);
        let verifying_key = signing_key.verifying_key();
        Self {
            signing_key,
            verifying_key,
        }
    }

    /// Create a key pair from a secret key seed (32 bytes, hex-encoded).
    pub fn from_seed_hex(hex: &str) -> Result<Self> {
        let bytes = hex::decode(hex).context("Invalid hex seed")?;
        let bytes: [u8; 32] = bytes
            .try_into()
            .map_err(|_| anyhow::anyhow!("Seed must be 32 bytes"))?;
        let signing_key = SigningKey::from_bytes(&bytes);
        let verifying_key = signing_key.verifying_key();
        Ok(Self {
            signing_key,
            verifying_key,
        })
    }

    /// Export the public key as hex.
    pub fn public_key_hex(&self) -> String {
        hex::encode(self.verifying_key.to_bytes())
    }

    /// Export the secret key as hex (for storage).
    pub fn secret_key_hex(&self) -> String {
        hex::encode(self.signing_key.to_bytes())
    }
}

impl fmt::Debug for AcpKeyPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AcpKeyPair")
            .field("public_key", &self.public_key_hex())
            .finish()
    }
}

// ─── Handshake ───────────────────────────────────────────────────────

/// A challenge-response authentication exchange.
pub struct AcpHandshake {
    local_key: AcpKeyPair,
    remote_key: Option<VerifyingKey>,
    session_key: Option<Vec<u8>>,
}

impl AcpHandshake {
    /// Create a new handshake with a local key pair.
    pub fn new(local_key: AcpKeyPair) -> Self {
        Self {
            local_key,
            remote_key: None,
            session_key: None,
        }
    }

    /// Generate a challenge for the remote peer.
    /// Returns the challenge (random bytes, hex-encoded) and the nonce used.
    pub fn generate_challenge(&self) -> (String, Vec<u8>) {
        let mut nonce = vec![0u8; 32];
        let mut rng = OsRng;
        rng.fill_bytes(&mut nonce);
        let challenge = hex::encode(&nonce);
        (challenge, nonce)
    }

    /// Sign a challenge to prove identity.
    /// Returns the signature (hex-encoded).
    pub fn sign_challenge(&self, challenge: &str) -> Result<String> {
        let challenge_bytes = hex::decode(challenge).context("Invalid challenge hex")?;
        let signature = self.local_key.signing_key.sign(&challenge_bytes);
        Ok(hex::encode(signature.to_bytes()))
    }

    /// Verify a peer's signature on a challenge.
    pub fn verify_challenge(
        &mut self,
        challenge: &str,
        signature_hex: &str,
        peer_public_key_hex: &str,
    ) -> Result<bool> {
        let challenge_bytes = hex::decode(challenge).context("Invalid challenge hex")?;
        let signature_bytes = hex::decode(signature_hex).context("Invalid signature hex")?;
        let pub_key_bytes = hex::decode(peer_public_key_hex).context("Invalid public key hex")?;

        let verifying_key = VerifyingKey::from_bytes(
            &pub_key_bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("Public key must be 32 bytes"))?,
        )
        .map_err(|_| anyhow::anyhow!("Invalid Ed25519 public key"))?;

        let signature = Signature::from_bytes(
            &signature_bytes
                .try_into()
                .map_err(|_| anyhow::anyhow!("Signature must be 64 bytes"))?,
        );

        let valid = verifying_key.verify(&challenge_bytes, &signature).is_ok();
        if valid {
            self.remote_key = Some(verifying_key);
        }
        Ok(valid)
    }

    /// Derive a shared session key using HMAC-SHA256.
    pub fn derive_session_key(&mut self, peer_public_key_hex: &str) -> Result<Vec<u8>> {
        let pub_key_bytes = hex::decode(peer_public_key_hex).context("Invalid public key hex")?;

        // Derive: HMAC-SHA256("acp-session-key-v1", local_secret || peer_public)
        let mut data = self.local_key.signing_key.to_bytes().to_vec();
        data.extend_from_slice(&pub_key_bytes);
        let session_key = hmac_sha256(b"acp-session-key-v1", &data).to_vec();

        self.session_key = Some(session_key.clone());
        Ok(session_key)
    }

    /// Verify a message authenticity using the session key.
    pub fn verify_message(&self, message: &[u8], tag: &[u8]) -> Result<bool> {
        let key = self
            .session_key
            .as_ref()
            .context("No session key derived yet")?;

        let expected = hmac_sha256(key, message);
        Ok(tag == expected.as_slice())
    }

    /// Get the remote peer's verified public key, if the handshake completed.
    pub fn remote_public_key(&self) -> Option<&VerifyingKey> {
        self.remote_key.as_ref()
    }

    /// Whether the handshake completed successfully.
    pub fn is_authenticated(&self) -> bool {
        self.remote_key.is_some() && self.session_key.is_some()
    }
}

// ─── Convenience functions ───────────────────────────────────────────

/// Perform a full handshake as the initiator.
///
/// 1. Generate a challenge
/// 2. Sign the challenge (send this to the responder)
/// 3. The responder returns their signature on the challenge
/// 4. Verify the responder's signature [... then call `complete_initiator`]
///
/// Returns (handshake, challenge, local_signature).
pub fn initiate_handshake(
    keypair: &AcpKeyPair,
) -> Result<(AcpHandshake, String, String)> {
    let handshake = AcpHandshake::new(AcpKeyPair {
        signing_key: SigningKey::from_bytes(&keypair.signing_key.to_bytes()),
        verifying_key: keypair.verifying_key,
    });

    // Step 1: Generate challenge
    let (challenge, _nonce) = handshake.generate_challenge();

    // Step 2: Sign the challenge (proof of identity for the responder)
    let signature = handshake.sign_challenge(&challenge)?;

    Ok((handshake, challenge, signature))
}

/// Complete the initiator handshake after receiving the responder's response.
///
/// 1. Verify the responder's signature on the original challenge
/// 2. Derive the shared session key
pub fn complete_initiator(
    handshake: &mut AcpHandshake,
    responder_signature: &str,
    responder_public_key_hex: &str,
    challenge: &str,
) -> Result<()> {
    // Verify the responder's signature
    let valid = handshake.verify_challenge(challenge, responder_signature, responder_public_key_hex)?;
    if !valid {
        anyhow::bail!("Responder signature verification failed");
    }

    // Derive session key
    handshake.derive_session_key(responder_public_key_hex)?;
    Ok(())
}

/// Perform a full handshake as the responder.
///
/// 1. Receive the challenge and the initiator's signature
/// 2. Verify the initiator's identity
/// 3. Sign the challenge to prove our identity
/// 4. Derive the shared session key
pub fn perform_responder_handshake(
    keypair: &AcpKeyPair,
    challenge: &str,
    initiator_signature: &str,
    initiator_public_key_hex: &str,
) -> Result<(AcpHandshake, String)> {
    let mut handshake = AcpHandshake::new(AcpKeyPair {
        signing_key: SigningKey::from_bytes(&keypair.signing_key.to_bytes()),
        verifying_key: keypair.verifying_key,
    });

    // Verify the initiator's signature
    let valid = handshake.verify_challenge(challenge, initiator_signature, initiator_public_key_hex)?;
    if !valid {
        anyhow::bail!("Initiator signature verification failed");
    }

    // Sign the challenge to prove our identity
    let our_signature = handshake.sign_challenge(challenge)?;

    // Derive session key
    handshake.derive_session_key(initiator_public_key_hex)?;

    Ok((handshake, our_signature))
}

// ─── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_key_pair_generation() {
        let kp = AcpKeyPair::generate();
        let pub_hex = kp.public_key_hex();
        assert_eq!(pub_hex.len(), 64); // 32 bytes = 64 hex chars
    }

    #[test]
    fn test_challenge_sign_verify() {
        let alice = AcpKeyPair::generate();
        let bob = AcpKeyPair::generate();

        let handshake = AcpHandshake::new(alice.clone());

        // Alice generates challenge for Bob
        let (challenge, _) = handshake.generate_challenge();

        // Bob signs the challenge
        let bob_handshake = AcpHandshake::new(bob.clone());
        let signature = bob_handshake.sign_challenge(&challenge).unwrap();

        // Alice verifies Bob's signature
        let mut alice_handshake = AcpHandshake::new(alice);
        let valid = alice_handshake
            .verify_challenge(&challenge, &signature, &bob.public_key_hex())
            .unwrap();
        assert!(valid);
    }

    #[test]
    fn test_full_initiator_handshake() {
        let alice = AcpKeyPair::generate();
        let bob = AcpKeyPair::generate();

        // Alice initiates
        let (mut alice_handshake, challenge, alice_sig) =
            initiate_handshake(&alice).unwrap();

        // Bob responds
        let (bob_handshake, bob_sig) = perform_responder_handshake(
            &bob,
            &challenge,
            &alice_sig,
            &alice.public_key_hex(),
        )
        .unwrap();

        // Alice completes with Bob's response
        complete_initiator(
            &mut alice_handshake,
            &bob_sig,
            &bob.public_key_hex(),
            &challenge,
        )
        .unwrap();

        assert!(alice_handshake.is_authenticated());
        assert!(bob_handshake.is_authenticated());
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let alice = AcpKeyPair::generate();
        let bob = AcpKeyPair::generate();
        let mallory = AcpKeyPair::generate();

        let handshake = AcpHandshake::new(alice.clone());
        let (challenge, _) = handshake.generate_challenge();

        // Mallory signs instead of Bob
        let mallory_handshake = AcpHandshake::new(mallory);
        let signature = mallory_handshake.sign_challenge(&challenge).unwrap();

        // Alice tries to verify with Bob's key — should fail
        let mut alice_handshake = AcpHandshake::new(alice);
        let valid = alice_handshake
            .verify_challenge(&challenge, &signature, &bob.public_key_hex())
            .unwrap();
        assert!(!valid);
    }
}
