use neo_crypto::{Crypto, Secp256r1Crypto};
use neo_primitives::UInt160;
use neo_protocol::witness::Witness;
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_smart_contract::helper::Helper;
use neo_vm::OpCode;
use std::convert::TryInto;

pub fn verify_witness_signature(
    witness: &Witness,
    hash_data: &[u8],
    account: &UInt160,
) -> CoreResult<bool> {
    let public_key = extract_public_key_from_verification_script(witness)?;
    let signature = extract_signature_from_invocation_script(witness)?;
    let signature_valid = verify_ecdsa_signature(hash_data, &signature, &public_key)?;
    if !signature_valid {
        return Ok(false);
    }
    let computed_account = compute_script_hash_from_public_key(&public_key)?;
    Ok(computed_account == *account)
}

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
    let mut sorted_keys = public_keys.to_vec();
    sorted_keys.sort();
    let total_keys = sorted_keys.len();
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
            &sorted_keys[key_index],
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

fn extract_public_key_from_verification_script(witness: &Witness) -> CoreResult<Vec<u8>> {
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

fn extract_signature_from_invocation_script(witness: &Witness) -> CoreResult<Vec<u8>> {
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

fn verify_ecdsa_signature(
    hash_data: &[u8],
    signature: &[u8],
    public_key: &[u8],
) -> CoreResult<bool> {
    let signature_bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| CoreError::invalid_data("Invalid signature length"))?;
    Secp256r1Crypto::verify_prehash(&Crypto::sha256(hash_data), &signature_bytes, public_key)
        .map_err(|e| CoreError::Cryptographic {
            message: format!("ECDSA verification failed: {e}"),
        })
}

fn compute_script_hash_from_public_key(public_key: &[u8]) -> CoreResult<UInt160> {
    let verification_script = create_verification_script_from_public_key(public_key)?;
    Ok(UInt160::from_script(&verification_script))
}

fn create_verification_script_from_public_key(public_key: &[u8]) -> CoreResult<Vec<u8>> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_verify_multi_signature() {
        let message = b"neo-multisig-test";
        let priv1 = Secp256r1Crypto::generate_private_key();
        let priv2 = Secp256r1Crypto::generate_private_key();
        let priv3 = Secp256r1Crypto::generate_private_key();
        let pub1 = Secp256r1Crypto::derive_public_key(&priv1).unwrap();
        let pub2 = Secp256r1Crypto::derive_public_key(&priv2).unwrap();
        let pub3 = Secp256r1Crypto::derive_public_key(&priv3).unwrap();
        let m = 2usize;
        let mut pairs = [(pub1, priv1), (pub2, priv2), (pub3, priv3)];
        pairs.sort_by(|(a, _), (b, _)| a.cmp(b));
        let public_keys: Vec<Vec<u8>> = pairs.iter().map(|(p, _)| p.clone()).collect();
        let verification_script = Helper::multi_sig_redeem_script(m, &public_keys);
        let account = UInt160::from_script(&verification_script);
        let signatures: Vec<Vec<u8>> = pairs
            .iter()
            .take(m)
            .map(|(_, pk)| {
                Secp256r1Crypto::sign_prehash(&Crypto::sha256(message), pk)
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
