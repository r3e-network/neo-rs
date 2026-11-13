//! Witness signature verification primitives.
//!
//! Migrated from `neo-core::witness::verification`. Rewrites applied:
//! - `super::Witness` -> `neo_protocol::payloads::Witness` (canonical payload owner)
//! - `crate::smart_contract::helper::Helper` -> `crate::helper::Helper`
//! - `crate::error::{CoreError, CoreResult}` -> this crate's own error type
//!
//! The canonical `Witness` payload is owned by `neo-protocol`, so these
//! VM/crypto-aware methods cannot be inherent methods here (that would
//! violate the orphan rule). They are exposed as an extension trait instead,
//! preserving the historical `witness.verify_*()` call syntax.

use crate::error::{CoreError, CoreResult};
use crate::helper::Helper;
use neo_crypto::{Crypto, Secp256r1Crypto};
use neo_primitives::UInt160;
use neo_protocol::payloads::Witness;
use neo_vm::OpCode;
use std::convert::TryInto;

/// Verifies the witness signature (production-ready implementation).
pub fn verify_witness_signature(
    witness: &Witness,
    hash_data: &[u8],
    account: &UInt160,
) -> CoreResult<bool> {
    // 1. Extract public key from verification script
    let public_key = extract_public_key_from_verification_script(witness)?;

    // 2. Extract signature from invocation script
    let signature = extract_signature_from_invocation_script(witness)?;

    // 3. Verify signature against hash_data using the public key
    let signature_valid = verify_ecdsa_signature(hash_data, &signature, &public_key)?;
    if !signature_valid {
        return Ok(false);
    }

    // 4. Verify that the public key corresponds to the account
    let computed_account = compute_script_hash_from_public_key(&public_key)?;
    Ok(computed_account == *account)
}

/// Verifies a multi-signature witness against the provided message.
pub fn verify_witness_multi_signature(
    message: &[u8],
    account: &UInt160,
    required_signatures: usize,
    public_keys: &[Vec<u8>],
    signatures: &[Vec<u8>],
) -> CoreResult<bool> {
    if required_signatures == 0
        || public_keys.is_empty()
        || required_signatures > public_keys.len()
        || signatures.len() != required_signatures
    {
        return Ok(false);
    }

    let Ok(script) = Helper::try_multi_sig_redeem_script(required_signatures, public_keys) else {
        return Ok(false);
    };

    if UInt160::from_script(&script) != *account {
        return Ok(false);
    }

    // CheckMultisig consumes public keys in verification-script order. Contract
    // construction sorts ECPoints by X then Y (C# ECPoint.CompareTo), which is
    // not necessarily the same as lexicographic compressed-key ordering. Keep
    // the caller's script order so each signature stays aligned with its slot.
    let total_keys = public_keys.len();
    let mut sig_index = 0usize;
    let mut key_index = 0usize;

    while sig_index < required_signatures && key_index < total_keys {
        let signature = &signatures[sig_index];
        if signature.len() != 64 {
            return Ok(false);
        }

        let signature_bytes: [u8; 64] = signature
            .as_slice()
            .try_into()
            .map_err(|_| CoreError::invalid_data("Invalid signature length"))?;

        let verified = Secp256r1Crypto::verify_prehash(
            &Crypto::sha256(message),
            &signature_bytes,
            &public_keys[key_index],
        )
        .map_err(|e| CoreError::Cryptographic {
            message: format!("ECDSA verification failed: {e}"),
        })?;

        if verified {
            sig_index += 1;
        }

        key_index += 1;

        if required_signatures - sig_index > total_keys - key_index {
            return Ok(false);
        }
    }

    Ok(sig_index == required_signatures)
}

/// Extracts public key from verification script (matches C# verification script parsing exactly).
fn extract_public_key_from_verification_script(witness: &Witness) -> Result<Vec<u8>, CoreError> {
    if !Helper::is_signature_contract(&witness.verification_script) {
        return Err(CoreError::Invalid {
            message: "Unsupported verification script format".to_string(),
        });
    }

    let public_key = witness.verification_script[2..35].to_vec();

    if public_key.len() != 33 || (public_key[0] != 0x02 && public_key[0] != 0x03) {
        return Err(CoreError::Invalid {
            message: "Invalid compressed public key format".to_string(),
        });
    }

    Ok(public_key)
}

/// Extracts signature from invocation script (matches C# signature extraction exactly).
fn extract_signature_from_invocation_script(witness: &Witness) -> Result<Vec<u8>, CoreError> {
    // Real C# Neo N3 implementation: Invocation script signature extraction

    if witness.invocation_script.len() != 66 {
        return Err(CoreError::Invalid {
            message: "Invalid invocation script length".to_string(),
        });
    }

    if witness.invocation_script[0] != OpCode::PUSHDATA1.byte()
        || witness.invocation_script[1] != 0x40
    {
        return Err(CoreError::Invalid {
            message: "Invalid invocation script format".to_string(),
        });
    }

    let signature = witness.invocation_script[2..66].to_vec();

    if signature.len() != 64 {
        return Err(CoreError::Invalid {
            message: "Invalid signature length".to_string(),
        });
    }

    Ok(signature)
}

/// Verifies ECDSA signature (matches C# ECDsa.VerifyData exactly).
fn verify_ecdsa_signature(
    hash_data: &[u8],
    signature: &[u8],
    public_key: &[u8],
) -> CoreResult<bool> {
    // Real C# Neo N3 implementation: ECDsa.VerifyData

    let signature_bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| CoreError::invalid_data("Invalid signature length"))?;

    Secp256r1Crypto::verify_prehash(&Crypto::sha256(hash_data), &signature_bytes, public_key)
        .map_err(|e| CoreError::Cryptographic {
            message: format!("ECDSA verification failed: {e}"),
        })
}

/// Computes script hash from public key (matches C# Contract.CreateSignatureContract exactly).
fn compute_script_hash_from_public_key(public_key: &[u8]) -> CoreResult<UInt160> {
    let verification_script = create_verification_script_from_public_key(public_key)?;
    Ok(UInt160::from_script(&verification_script))
}

/// Creates verification script from public key (matches C# Contract.CreateSignatureRedeemScript exactly).
fn create_verification_script_from_public_key(public_key: &[u8]) -> Result<Vec<u8>, CoreError> {
    if public_key.len() != 33 {
        return Err(CoreError::Invalid {
            message: "Public key must be 33 bytes (compressed)".to_string(),
        });
    }

    if public_key[0] != 0x02 && public_key[0] != 0x03 {
        return Err(CoreError::Invalid {
            message: "Invalid compressed public key format".to_string(),
        });
    }

    Ok(Helper::signature_redeem_script(public_key))
}

/// Witness verification helpers over the canonical `neo-protocol` payload.
pub trait WitnessExt {
    /// Verifies the witness signature against `hash_data` for `account`.
    fn verify_signature(&self, hash_data: &[u8], account: &UInt160) -> CoreResult<bool>;

    /// Verifies a multi-signature witness against the provided message.
    fn verify_multi_signature(
        &self,
        message: &[u8],
        account: &UInt160,
        required_signatures: usize,
        public_keys: &[Vec<u8>],
        signatures: &[Vec<u8>],
    ) -> CoreResult<bool>;
}

impl WitnessExt for Witness {
    fn verify_signature(&self, hash_data: &[u8], account: &UInt160) -> CoreResult<bool> {
        verify_witness_signature(self, hash_data, account)
    }

    fn verify_multi_signature(
        &self,
        message: &[u8],
        account: &UInt160,
        required_signatures: usize,
        public_keys: &[Vec<u8>],
        signatures: &[Vec<u8>],
    ) -> CoreResult<bool> {
        verify_witness_multi_signature(
            message,
            account,
            required_signatures,
            public_keys,
            signatures,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_crypto::ECPoint;

    #[test]
    fn test_witness_verify_multi_signature() {
        let message = b"neo-multisig-test";
        let candidates: Vec<([u8; 32], Vec<u8>)> = (1u8..=32)
            .map(|value| {
                let private_key = [value; 32];
                let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
                (private_key, public_key)
            })
            .collect();

        // Find fixed keys whose C# ECPoint ordering differs from compressed
        // byte ordering. This specifically guards against re-sorting keys in
        // the verifier after the signatures have been assigned script slots.
        let (left, right) = 'search: {
            for (left_index, left) in candidates.iter().enumerate() {
                for right in candidates.iter().skip(left_index + 1) {
                    let point_order = ECPoint::from_bytes(&left.1)
                        .unwrap()
                        .cmp(&ECPoint::from_bytes(&right.1).unwrap());
                    if point_order != left.1.cmp(&right.1) {
                        break 'search (left.clone(), right.clone());
                    }
                }
            }
            panic!("fixed P-256 key set must contain differing orderings");
        };

        let m = 2usize;
        let mut pairs = [left, right];
        pairs.sort_by(|(_, left), (_, right)| {
            ECPoint::from_bytes(left)
                .unwrap()
                .cmp(&ECPoint::from_bytes(right).unwrap())
        });

        let public_keys: Vec<Vec<u8>> = pairs.iter().map(|(_, public)| public.clone()).collect();
        let verification_script = Helper::multi_sig_redeem_script(m, &public_keys);
        let account = UInt160::from_script(&verification_script);

        let signatures: Vec<Vec<u8>> = pairs
            .iter()
            .map(|(private_key, _)| {
                Secp256r1Crypto::sign_prehash(&Crypto::sha256(message), private_key)
                    .unwrap()
                    .to_vec()
            })
            .collect();

        let ok = verify_witness_multi_signature(message, &account, m, &public_keys, &signatures)
            .unwrap();
        assert!(ok);

        let bad =
            verify_witness_multi_signature(message, &account, m, &public_keys, &signatures[..1])
                .unwrap();
        assert!(!bad);
    }
}
