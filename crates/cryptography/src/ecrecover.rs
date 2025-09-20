//!
//! Matches C# Neo cryptography ECRecover functionality exactly

// Use standard error types for now
type CryptographyResult<T> = Result<T, CryptographyError>;

#[derive(Debug)]
pub enum CryptographyError {
    InvalidInput(String),
}
use secp256k1::{ecdsa::RecoverableSignature, ecdsa::RecoveryId, Message, PublicKey, Secp256k1};

/// ECRecover functionality (matches C# ECRecover exactly)
pub struct ECRecover;

impl ECRecover {
    /// Recover public key from signature and message hash (matches C# ECRecover.TryRecover)
    pub fn try_recover(
        message_hash: &[u8],
        signature: &[u8],
        recovery_id: u8,
    ) -> CryptographyResult<Vec<u8>> {
        if message_hash.len() != 32 {
            return Err(CryptographyError::InvalidInput(
                "Message hash must be 32 bytes".to_string(),
            ));
        }

        if signature.len() != 64 {
            return Err(CryptographyError::InvalidInput(
                "Signature must be 64 bytes".to_string(),
            ));
        }

        if recovery_id > 3 {
            return Err(CryptographyError::InvalidInput(
                "Recovery ID must be 0-3".to_string(),
            ));
        }

        let secp = Secp256k1::new();

        let message = Message::from_digest_slice(message_hash)
            .map_err(|e| CryptographyError::InvalidInput(format!("Invalid message hash: {e}")))?;

        let recovery_id = RecoveryId::from_i32(recovery_id as i32)
            .map_err(|e| CryptographyError::InvalidInput(format!("Invalid recovery ID: {e}")))?;

        let mut sig_bytes = [0u8; 64];
        sig_bytes.copy_from_slice(signature);

        let recoverable_sig = RecoverableSignature::from_compact(&sig_bytes, recovery_id)
            .map_err(|e| CryptographyError::InvalidInput(format!("Invalid signature: {e}")))?;

        let recovered_pubkey = secp
            .recover_ecdsa(&message, &recoverable_sig)
            .map_err(|e| CryptographyError::InvalidInput(format!("Recovery failed: {e}")))?;

        Ok(recovered_pubkey.serialize_uncompressed().to_vec())
    }

    /// Recover public key with automatic recovery ID detection
    pub fn recover_public_key(
        message_hash: &[u8],
        signature: &[u8],
    ) -> CryptographyResult<Vec<u8>> {
        for recovery_id in 0..4 {
            if let Ok(public_key) = Self::try_recover(message_hash, signature, recovery_id) {
                return Ok(public_key);
            }
        }

        Err(CryptographyError::InvalidInput(
            "Could not recover public key with any recovery ID".to_string(),
        ))
    }

    /// Verify signature using recovered public key (matches C# verification)
    pub fn verify_signature(
        message_hash: &[u8],
        signature: &[u8],
        expected_pubkey: &[u8],
    ) -> CryptographyResult<bool> {
        let recovered_pubkey = Self::recover_public_key(message_hash, signature)?;

        if recovered_pubkey.len() != expected_pubkey.len() {
            return Ok(false);
        }

        if expected_pubkey.len() == 33 {
            let pubkey = PublicKey::from_slice(&recovered_pubkey).map_err(|e| {
                CryptographyError::InvalidInput(format!("Invalid recovered key: {e}"))
            })?;
            let compressed = pubkey.serialize();
            Ok(compressed.to_vec() == expected_pubkey)
        } else if expected_pubkey.len() == 65 {
            Ok(recovered_pubkey == expected_pubkey)
        } else {
            Err(CryptographyError::InvalidInput(
                "Expected public key must be 33 or 65 bytes".to_string(),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::{
        ecdsa::RecoverableSignature, Message, Secp256k1, SecretKey as Secp256k1SecretKey,
    };
    use sha2::{Digest, Sha256};

    fn sample_message_hash() -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"neo-ecrecover-test");
        hasher.finalize().into()
    }

    fn sample_signature() -> ([u8; 32], [u8; 32], u8, Vec<u8>) {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::from_slice(&[0x11u8; 32]).expect("valid secret key");
        let public_key = secp256k1::PublicKey::from_secret_key(&secp, &secret_key);

        let message_hash = sample_message_hash();
        let message = Message::from_digest_slice(&message_hash).expect("32-byte digest");

        let recoverable: RecoverableSignature = secp.sign_ecdsa_recoverable(&message, &secret_key);
        let (rec_id, compact) = recoverable.serialize_compact();

        let mut r = [0u8; 32];
        let mut s = [0u8; 32];
        r.copy_from_slice(&compact[..32]);
        s.copy_from_slice(&compact[32..]);

        (
            r,
            s,
            rec_id.to_i32() as u8,
            public_key.serialize_uncompressed().to_vec(),
        )
    }

    #[test]
    fn test_ecrecover_basic() {
        let (r, s, recovery_id, expected_pubkey) = sample_signature();
        let message_hash = sample_message_hash();

        let mut signature = [0u8; 64];
        signature[..32].copy_from_slice(&r);
        signature[32..].copy_from_slice(&s);

        let recovered = ECRecover::try_recover(&message_hash, &signature, recovery_id)
            .expect("public key recovery to succeed");

        assert_eq!(recovered.len(), 65);
        assert_eq!(recovered, expected_pubkey);
    }

    #[test]
    fn test_ecrecover_verification() {
        let (r, s, _recovery_id, expected_uncompressed) = sample_signature();
        let message_hash = sample_message_hash();

        let mut signature = [0u8; 64];
        signature[..32].copy_from_slice(&r);
        signature[32..].copy_from_slice(&s);

        let is_valid =
            ECRecover::verify_signature(&message_hash, &signature, &expected_uncompressed)
                .expect("verification");
        assert!(is_valid);

        let mut tampered_hash = message_hash;
        tampered_hash[0] ^= 0xFF;
        let tampered =
            ECRecover::verify_signature(&tampered_hash, &signature, &expected_uncompressed)
                .expect("verification");
        assert!(!tampered);
    }
}
