//! Comprehensive tests for CryptoError types

use neo_crypto::error::{CryptoError, CryptoResult};

#[test]
fn test_invalid_argument_error() {
    let err = CryptoError::invalid_argument("test message");
    assert!(matches!(err, CryptoError::InvalidArgument { .. }));
    assert_eq!(err.to_string(), "Invalid argument: test message");
}

#[test]
fn test_invalid_key_error() {
    let err = CryptoError::invalid_key("invalid key format");
    assert!(matches!(err, CryptoError::InvalidKey { .. }));
    assert_eq!(err.to_string(), "Invalid key: invalid key format");
}

#[test]
fn test_invalid_signature_error() {
    let err = CryptoError::invalid_signature("signature verification failed");
    assert!(matches!(err, CryptoError::InvalidSignature { .. }));
    assert_eq!(
        err.to_string(),
        "Invalid signature: signature verification failed"
    );
}

#[test]
fn test_invalid_point_error() {
    let err = CryptoError::invalid_point("point not on curve");
    assert!(matches!(err, CryptoError::InvalidPoint { .. }));
    assert_eq!(err.to_string(), "Invalid curve point: point not on curve");
}

#[test]
fn test_hash_error() {
    let err = CryptoError::HashError {
        message: "hash computation failed".to_string(),
    };
    assert_eq!(err.to_string(), "Hash error: hash computation failed");
}

#[test]
fn test_encoding_error() {
    let err = CryptoError::encoding_error("invalid base58");
    assert!(matches!(err, CryptoError::EncodingError { .. }));
    assert_eq!(err.to_string(), "Encoding error: invalid base58");
}

#[test]
fn test_key_generation_failed_error() {
    let err = CryptoError::key_generation_failed("exhausted retries");
    assert!(matches!(err, CryptoError::KeyGenerationFailed { .. }));
    assert_eq!(err.to_string(), "Key generation failed: exhausted retries");
}

#[test]
fn test_crypto_error_clone() {
    let err1 = CryptoError::invalid_argument("test");
    let err2 = err1.clone();
    assert_eq!(err1, err2);
}

#[test]
fn test_crypto_error_debug() {
    let err = CryptoError::invalid_key("test key");
    let debug_str = format!("{err:?}");
    assert!(debug_str.contains("InvalidKey"));
}

#[test]
fn test_crypto_error_partial_eq() {
    let err1 = CryptoError::invalid_argument("test");
    let err2 = CryptoError::invalid_argument("test");
    let err3 = CryptoError::invalid_argument("different");

    assert_eq!(err1, err2);
    assert_ne!(err1, err3);
}

#[test]
fn test_crypto_result_ok() {
    let result: CryptoResult<i32> = Ok(42);
    assert_eq!(result, Ok(42));
}

#[test]
fn test_crypto_result_err() {
    let result: CryptoResult<i32> = Err(CryptoError::invalid_argument("test"));
    assert!(result.is_err());
}

#[test]
fn test_from_secp256k1_error() {
    let secp_err = secp256k1::Error::InvalidPublicKey;
    let crypto_err: CryptoError = secp_err.into();
    assert!(matches!(crypto_err, CryptoError::InvalidKey { .. }));
}

#[test]
fn test_from_p256_ecdsa_error() {
    use p256::ecdsa::signature::Error as SignatureError;
    let sig_err = SignatureError::new();
    let crypto_err: CryptoError = sig_err.into();
    assert!(matches!(crypto_err, CryptoError::InvalidSignature { .. }));
}

#[test]
fn test_from_bs58_decode_error() {
    let decode_result = bs58::decode("invalid!!!").into_vec();
    assert!(decode_result.is_err());

    if let Err(e) = decode_result {
        let crypto_err: CryptoError = e.into();
        assert!(matches!(crypto_err, CryptoError::EncodingError { .. }));
        assert!(crypto_err.to_string().contains("Base58"));
    }
}

#[test]
fn test_from_hex_error() {
    let decode_result = hex::decode("xyz");
    assert!(decode_result.is_err());

    if let Err(e) = decode_result {
        let crypto_err: CryptoError = e.into();
        assert!(matches!(crypto_err, CryptoError::EncodingError { .. }));
        assert!(crypto_err.to_string().contains("Hex"));
    }
}

#[test]
fn test_error_message_with_string() {
    let err = CryptoError::invalid_argument(String::from("owned string"));
    assert_eq!(err.to_string(), "Invalid argument: owned string");
}

#[test]
fn test_error_message_with_str() {
    let err = CryptoError::invalid_key("borrowed str");
    assert_eq!(err.to_string(), "Invalid key: borrowed str");
}

#[test]
fn test_multiple_error_types() {
    let errors = vec![
        CryptoError::invalid_argument("arg"),
        CryptoError::invalid_key("key"),
        CryptoError::invalid_signature("sig"),
        CryptoError::invalid_point("point"),
        CryptoError::encoding_error("enc"),
        CryptoError::key_generation_failed("gen"),
    ];

    assert_eq!(errors.len(), 6);
    for err in errors {
        assert!(!err.to_string().is_empty());
    }
}

#[test]
fn test_error_in_result_chain() {
    fn inner() -> CryptoResult<i32> {
        Err(CryptoError::invalid_argument("inner error"))
    }

    fn outer() -> CryptoResult<i32> {
        inner()?;
        Ok(42)
    }

    let result = outer();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("inner error"));
}
