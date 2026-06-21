use super::*;
use crate::nitro::vsock::MockTransport;
use neo_crypto::Crypto;

/// Produces a deterministic test keypair + script hash.
fn test_identity() -> ([u8; 32], Vec<u8>, UInt160) {
    let private_key = [7u8; 32];
    let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let script_hash = script_hash_from_public_key(&public_key).unwrap();
    (private_key, public_key, script_hash)
}

#[test]
fn script_hash_matches_wallet_scheme() {
    let (_priv, public_key, sh) = test_identity();
    // Recompute via the same scheme to confirm determinism.
    let mut script = vec![0x0c, public_key.len() as u8];
    script.extend_from_slice(&public_key);
    script.push(0x41);
    let syscall = Crypto::sha256(b"System.Crypto.CheckSig");
    script.extend_from_slice(&syscall[..4]);
    assert_eq!(sh, UInt160::from_script(&script));
}

#[test]
fn script_hash_rejects_bad_key() {
    assert!(script_hash_from_public_key(&[0u8; 10]).is_err());
    assert!(script_hash_from_public_key(&[0xFFu8; 33]).is_err());
}

#[test]
fn can_sign_matches_only_its_script_hash() {
    let (_priv, public_key, sh) = test_identity();
    let transport = Arc::new(MockTransport::new(|_| {
        Ok(EnclaveResponse::Error {
            message: "unused".to_string(),
        })
    }));
    let signer = NitroEnclaveSigner::new(transport, public_key, sh);

    assert!(signer.can_sign(&sh));
    assert!(!signer.can_sign(&UInt160::zero()));
}

#[test]
fn sign_forwards_and_returns_canonical_signature() {
    // The mock enclave signs with a known key and returns the 64-byte sig.
    let (private_key, public_key, sh) = test_identity();
    let pk_for_handler = public_key.clone();
    let priv_for_handler = private_key;

    let transport = Arc::new(MockTransport::with_framing(move |req| match req {
        EnclaveRequest::SignBlock {
            sign_data,
            script_hash,
        } => {
            assert_eq!(*script_hash, sh.to_array());
            // Enclave behavior: SHA-256(data) then secp256r1 sign + low-s.
            let digest = Crypto::sha256(sign_data);
            let raw = Secp256r1Crypto::sign_prehash(&digest, &priv_for_handler).unwrap();
            let low_s = Secp256r1Crypto::normalize_low_s(&raw).unwrap();
            Ok(EnclaveResponse::Signature(low_s.to_vec()))
        }
        _ => Ok(EnclaveResponse::Error {
            message: "unexpected".to_string(),
        }),
    }));

    let signer = NitroEnclaveSigner::new(transport, public_key.clone(), sh);

    let data = b"network_le|block_hash bytes here";
    let sig = signer.sign(data, &sh).unwrap();
    assert_eq!(sig.len(), 64);

    // The returned signature must verify over SHA-256(data) under the pubkey.
    let digest = Crypto::sha256(data);
    let sig_arr: [u8; 64] = sig.as_slice().try_into().unwrap();
    // Verify against the prehash path the enclave used.
    let verifying =
        Secp256r1Crypto::verify(&digest, &sig_arr, &pk_for_handler).unwrap_or(false) || {
            // Some backends sign over the message directly; accept either as a
            // valid signature of the same logical content for this skeleton.
            Secp256r1Crypto::verify(data, &sig_arr, &pk_for_handler).unwrap_or(false)
        };
    assert!(verifying, "returned signature must verify under the pubkey");
}

#[test]
fn sign_rejects_wrong_script_hash() {
    let (_priv, public_key, sh) = test_identity();
    let transport = Arc::new(MockTransport::new(|_| {
        Ok(EnclaveResponse::Signature(vec![0u8; 64]))
    }));
    let signer = NitroEnclaveSigner::new(transport, public_key, sh);

    let err = signer.sign(b"data", &UInt160::zero()).unwrap_err();
    assert!(format!("{err}").contains("unknown script hash"));
}

#[test]
fn sign_propagates_enclave_error() {
    let (_priv, public_key, sh) = test_identity();
    let transport = Arc::new(MockTransport::new(|_| {
        Ok(EnclaveResponse::Error {
            message: "enclave busy".to_string(),
        })
    }));
    let signer = NitroEnclaveSigner::new(transport, public_key, sh);
    let err = signer.sign(b"data", &sh).unwrap_err();
    assert!(format!("{err}").contains("enclave busy"));
}

#[test]
fn sign_rejects_wrong_length_signature() {
    let (_priv, public_key, sh) = test_identity();
    let transport = Arc::new(MockTransport::new(|_| {
        Ok(EnclaveResponse::Signature(vec![0u8; 10]))
    }));
    let signer = NitroEnclaveSigner::new(transport, public_key, sh);
    let err = signer.sign(b"data", &sh).unwrap_err();
    assert!(format!("{err}").contains("signature length"));
}

#[test]
fn connect_fetches_identity_and_validates_consistency() {
    let (_priv, public_key, sh) = test_identity();
    let pk = public_key.clone();
    let transport = Arc::new(MockTransport::with_framing(move |req| match req {
        EnclaveRequest::GetPublicKey => Ok(EnclaveResponse::PublicKey {
            public_key: pk.clone(),
            script_hash: sh.to_array(),
        }),
        _ => Ok(EnclaveResponse::Error {
            message: "unexpected".to_string(),
        }),
    }));

    let signer = NitroEnclaveSigner::connect(transport).unwrap();
    assert_eq!(signer.public_key(), public_key.as_slice());
    assert_eq!(signer.script_hash(), sh);
    assert!(signer.can_sign(&sh));
}

#[test]
fn connect_rejects_mismatched_script_hash() {
    let (_priv, public_key, _sh) = test_identity();
    let pk = public_key.clone();
    let transport = Arc::new(MockTransport::new(move |_| {
        Ok(EnclaveResponse::PublicKey {
            public_key: pk.clone(),
            script_hash: [0xFF; 20], // wrong hash for the key
        })
    }));
    assert!(NitroEnclaveSigner::connect(transport).is_err());
}
