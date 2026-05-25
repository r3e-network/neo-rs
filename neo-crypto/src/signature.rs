//! Signature and key helpers used by Neo cryptographic APIs.

use crate::error::CryptoError;
use crate::{Crypto, CryptoResult, ECCurve, ECPoint, HashAlgorithm};
use core::convert::TryFrom;
use ed25519_dalek::{
    Signature as Ed25519Signature, SigningKey as Ed25519SigningKey,
    VerifyingKey as Ed25519VerifyingKey,
};
use ed25519_dalek::{Signer as _, Verifier as _};
use p256::{
    ecdsa::{
        signature::hazmat::{PrehashSigner, PrehashVerifier},
        Signature, SigningKey, VerifyingKey,
    },
    PublicKey as P256PublicKey, SecretKey as P256SecretKey,
};
use rand::{rngs::OsRng, RngCore};
use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Message, PublicKey as Secp256k1PublicKey, Secp256k1, SecretKey as Secp256k1SecretKey,
};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

/// ECDSA operations for secp256k1 (Bitcoin's curve).
pub struct Secp256k1Crypto;

/// Maximum attempts for key generation to prevent infinite loops in case of RNG failure.
const MAX_KEY_GEN_ATTEMPTS: usize = 1000;

impl Secp256k1Crypto {
    /// Generates a new random private key.
    ///
    /// # Errors
    /// Returns an error if a valid key cannot be generated after `MAX_KEY_GEN_ATTEMPTS` attempts.
    /// This should only occur if the system RNG is misbehaving.
    pub fn generate_private_key() -> CryptoResult<[u8; 32]> {
        let mut rng = OsRng;
        for _ in 0..MAX_KEY_GEN_ATTEMPTS {
            let mut candidate = Zeroizing::new([0u8; 32]);
            rng.fill_bytes(candidate.as_mut());
            if let Ok(secret_key) = Secp256k1SecretKey::from_slice(candidate.as_ref()) {
                return Ok(secret_key.secret_bytes());
            }
        }
        Err(CryptoError::key_generation_failed(format!(
            "Failed to generate valid secp256k1 private key after {MAX_KEY_GEN_ATTEMPTS} attempts"
        )))
    }

    /// Derives public key from private key.
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<[u8; 33]> {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::from_slice(private_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let public_key = Secp256k1PublicKey::from_secret_key(&secp, &secret_key);
        Ok(public_key.serialize())
    }

    /// Signs a message with secp256k1.
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 64]> {
        let secp = Secp256k1::new();
        let secret_key = Secp256k1SecretKey::from_slice(private_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;

        let message_hash = Sha256::digest(message);
        let message = Message::from_digest_slice(&message_hash)
            .map_err(|e| CryptoError::invalid_argument(format!("Invalid message: {e}")))?;

        let signature = secp.sign_ecdsa(&message, &secret_key);
        Ok(signature.serialize_compact())
    }

    /// Verifies a secp256k1 signature.
    pub fn verify(
        message: &[u8],
        signature: &[u8; 64],
        public_key: &[u8; 33],
    ) -> CryptoResult<bool> {
        let secp = Secp256k1::verification_only();
        let public_key = Secp256k1PublicKey::from_slice(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;

        let message_hash = Sha256::digest(message);
        let message = Message::from_digest_slice(&message_hash)
            .map_err(|e| CryptoError::invalid_argument(format!("Invalid message: {e}")))?;

        let signature = secp256k1::ecdsa::Signature::from_compact(signature)
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;

        Ok(secp.verify_ecdsa(&message, &signature, &public_key).is_ok())
    }

    /// Recovers a compressed secp256k1 public key from a message hash and signature.
    ///
    /// Accepts 65-byte (r||s||v) or 64-byte EIP-2098 compact signatures.
    pub fn recover_public_key(message_hash: &[u8], signature: &[u8]) -> CryptoResult<Vec<u8>> {
        if signature.len() != 65 && signature.len() != 64 {
            return Err(CryptoError::invalid_signature(
                "Signature must be 65 or 64 bytes",
            ));
        }
        if message_hash.len() != 32 {
            return Err(CryptoError::invalid_argument(
                "Message hash must be 32 bytes",
            ));
        }

        let msg = Message::from_digest_slice(message_hash)
            .map_err(|e| CryptoError::invalid_argument(format!("Invalid message hash: {e}")))?;

        let (rec_id, sig_bytes) = if signature.len() == 65 {
            let rec = signature[64];
            let rec_id = if rec >= 27 { rec - 27 } else { rec };
            if rec_id > 3 {
                return Err(CryptoError::invalid_signature(
                    "Recovery id must be in range 0..3",
                ));
            }
            (rec_id, signature[..64].to_vec())
        } else {
            let mut sig = signature.to_vec();
            let y_parity = (sig[32] & 0x80) != 0;
            sig[32] &= 0x7f;
            let rec_id = u8::from(y_parity);
            (rec_id, sig)
        };

        let rec_id = RecoveryId::from_i32(i32::from(rec_id))
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid recovery id: {e}")))?;
        let recoverable = RecoverableSignature::from_compact(&sig_bytes, rec_id).map_err(|e| {
            CryptoError::invalid_signature(format!("Invalid recoverable signature: {e}"))
        })?;

        let secp = Secp256k1::new();
        let public_key = secp
            .recover_ecdsa(&msg, &recoverable)
            .map_err(|e| CryptoError::invalid_key(format!("Failed to recover public key: {e}")))?;

        Ok(public_key.serialize().to_vec())
    }
}

/// ECDSA operations for secp256r1 (P-256, Neo's primary curve).
pub struct Secp256r1Crypto;

/// Signature prefix used by NeoFS ECDSA_SHA512 signatures.
pub const NEOFS_ECDSA_SHA512_PREFIX: u8 = 0x04;

/// Serialized NeoFS ECDSA_SHA512 signature length: one prefix byte plus raw P-256 ECDSA.
pub const NEOFS_ECDSA_SHA512_SIGNATURE_LEN: usize = 65;

impl Secp256r1Crypto {
    /// Generates a new random private key.
    pub fn generate_private_key() -> [u8; 32] {
        let secret_key = P256SecretKey::random(&mut OsRng);
        let bytes = secret_key.to_bytes();
        let mut key = [0u8; 32];
        key.copy_from_slice(bytes.as_slice());
        key
    }

    /// Derives public key from private key.
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<Vec<u8>> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let verifying_key = VerifyingKey::from(&signing_key);
        Ok(verifying_key.to_encoded_point(true).as_bytes().to_vec())
    }

    /// Signs a message with secp256r1.
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 64]> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let signature: Signature = signing_key.sign(message);
        let bytes: [u8; 64] = signature.to_bytes().into();
        Ok(bytes)
    }

    /// Verifies a secp256r1 signature.
    pub fn verify(message: &[u8], signature: &[u8; 64], public_key: &[u8]) -> CryptoResult<bool> {
        let public_key = P256PublicKey::from_sec1_bytes(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
        let verifying_key = VerifyingKey::from(public_key);

        let signature = Signature::try_from(signature.as_slice())
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;

        Ok(verifying_key.verify(message, &signature).is_ok())
    }

    /// Signs NeoFS data using P-256 over a SHA-512 prehash.
    ///
    /// NeoFS serializes this signature as `0x04 || raw_ecdsa_signature`.
    pub fn sign_neofs_sha512(
        data: &[u8],
        private_key: &[u8; 32],
    ) -> CryptoResult<[u8; NEOFS_ECDSA_SHA512_SIGNATURE_LEN]> {
        let signing_key = SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let digest = Crypto::sha512(data);
        let signature: Signature = signing_key
            .sign_prehash(&digest)
            .map_err(|e| CryptoError::invalid_signature(format!("Failed to sign: {e}")))?;

        let mut output = [0u8; NEOFS_ECDSA_SHA512_SIGNATURE_LEN];
        output[0] = NEOFS_ECDSA_SHA512_PREFIX;
        output[1..].copy_from_slice(&signature.to_bytes());
        Ok(output)
    }

    /// Verifies a NeoFS P-256 signature over a SHA-512 prehash.
    ///
    /// The verifier preserves current NeoFS behavior by requiring a 65-byte signature while
    /// ignoring the first byte instead of enforcing the `0x04` prefix.
    pub fn verify_neofs_sha512(
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
    ) -> CryptoResult<bool> {
        if signature.len() != NEOFS_ECDSA_SHA512_SIGNATURE_LEN {
            return Err(CryptoError::invalid_signature(format!(
                "NeoFS signature must be {NEOFS_ECDSA_SHA512_SIGNATURE_LEN} bytes"
            )));
        }

        let public_key = P256PublicKey::from_sec1_bytes(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
        let verifying_key = VerifyingKey::from(public_key);
        let signature = Signature::try_from(&signature[1..])
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;
        let digest = Crypto::sha512(data);
        Ok(verifying_key.verify_prehash(&digest, &signature).is_ok())
    }
}

/// Ed25519 operations.
pub struct Ed25519Crypto;

impl Ed25519Crypto {
    /// Generates a new random private key using cryptographically secure RNG.
    pub fn generate_private_key() -> [u8; 32] {
        let signing_key = Ed25519SigningKey::generate(&mut OsRng);
        signing_key.to_bytes()
    }

    /// Derives public key from private key.
    pub fn derive_public_key(private_key: &[u8; 32]) -> CryptoResult<[u8; 32]> {
        let signing_key = Ed25519SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        Ok(signing_key.verifying_key().to_bytes())
    }

    /// Signs a message with Ed25519.
    pub fn sign(message: &[u8], private_key: &[u8; 32]) -> CryptoResult<[u8; 64]> {
        let signing_key = Ed25519SigningKey::try_from(private_key.as_slice())
            .map_err(|e| CryptoError::invalid_key(format!("Invalid private key: {e}")))?;
        let signature = signing_key.sign(message);
        Ok(signature.to_bytes())
    }

    /// Verifies an Ed25519 signature.
    pub fn verify(
        message: &[u8],
        signature: &[u8; 64],
        public_key: &[u8; 32],
    ) -> CryptoResult<bool> {
        let verifying_key = Ed25519VerifyingKey::from_bytes(public_key)
            .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
        let signature = Ed25519Signature::try_from(signature.as_slice())
            .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;

        Ok(verifying_key.verify_strict(message, &signature).is_ok())
    }
}

fn verify_ecdsa_raw64_with_hash(
    data: &[u8],
    signature: &[u8; 64],
    public_key: &[u8],
    curve: ECCurve,
    hash_algorithm: HashAlgorithm,
) -> CryptoResult<bool> {
    match (curve, hash_algorithm) {
        (ECCurve::Secp256k1, HashAlgorithm::Keccak256) => {
            let sig = secp256k1::ecdsa::Signature::from_compact(signature)
                .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;
            let pubkey = Secp256k1PublicKey::from_slice(public_key)
                .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
            let hash = Crypto::keccak256(data);
            let msg = Message::from_digest_slice(&hash)
                .map_err(|e| CryptoError::invalid_argument(format!("Invalid message: {e}")))?;
            Ok(Secp256k1::verification_only()
                .verify_ecdsa(&msg, &sig, &pubkey)
                .is_ok())
        }
        (ECCurve::Secp256r1, HashAlgorithm::Keccak256) => {
            let public_key = P256PublicKey::from_sec1_bytes(public_key)
                .map_err(|e| CryptoError::invalid_key(format!("Invalid public key: {e}")))?;
            let verifying_key = VerifyingKey::from(public_key);
            let signature = Signature::try_from(signature.as_slice())
                .map_err(|e| CryptoError::invalid_signature(format!("Invalid signature: {e}")))?;
            let hash = Crypto::keccak256(data);
            Ok(verifying_key.verify_prehash(&hash, &signature).is_ok())
        }
        (ECCurve::Secp256k1, _) => {
            let public_key: [u8; 33] = public_key
                .try_into()
                .map_err(|_| CryptoError::invalid_key("Invalid public key length"))?;
            Secp256k1Crypto::verify(data, signature, &public_key)
        }
        (ECCurve::Secp256r1, _) => Secp256r1Crypto::verify(data, signature, public_key),
        (ECCurve::Ed25519, _) => Err(CryptoError::invalid_argument(
            "Ed25519 is not an ECDSA curve",
        )),
    }
}

/// ECDSA operations wrapper.
pub struct ECDsa;

impl ECDsa {
    /// Signs data with ECDSA.
    pub fn sign(data: &[u8], private_key: &[u8; 32], curve: ECCurve) -> CryptoResult<[u8; 64]> {
        match curve {
            ECCurve::Secp256k1 => Secp256k1Crypto::sign(data, private_key),
            ECCurve::Secp256r1 => Secp256r1Crypto::sign(data, private_key),
            ECCurve::Ed25519 => Ed25519Crypto::sign(data, private_key),
        }
    }

    /// Verifies ECDSA signature.
    pub fn verify(
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
        curve: ECCurve,
    ) -> CryptoResult<bool> {
        match curve {
            ECCurve::Secp256k1 => {
                if signature.len() != 64 || public_key.len() != 33 {
                    return Err(CryptoError::invalid_argument(
                        "Invalid signature or public key length",
                    ));
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| CryptoError::invalid_signature("Invalid signature length"))?;
                verify_ecdsa_raw64_with_hash(
                    data,
                    &sig_bytes,
                    public_key,
                    ECCurve::Secp256k1,
                    HashAlgorithm::Sha256,
                )
            }
            ECCurve::Secp256r1 => {
                if signature.len() != 64 {
                    return Err(CryptoError::invalid_signature("Invalid signature length"));
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| CryptoError::invalid_signature("Invalid signature length"))?;
                verify_ecdsa_raw64_with_hash(
                    data,
                    &sig_bytes,
                    public_key,
                    ECCurve::Secp256r1,
                    HashAlgorithm::Sha256,
                )
            }
            ECCurve::Ed25519 => {
                if signature.len() != 64 || public_key.len() != 32 {
                    return Err(CryptoError::invalid_argument(
                        "Invalid signature or public key length",
                    ));
                }
                let sig_bytes: [u8; 64] = signature
                    .try_into()
                    .map_err(|_| CryptoError::invalid_signature("Invalid signature length"))?;
                let pub_bytes: [u8; 32] = public_key
                    .try_into()
                    .map_err(|_| CryptoError::invalid_key("Invalid public key length"))?;
                Ed25519Crypto::verify(data, &sig_bytes, &pub_bytes)
            }
        }
    }
}

/// ECC operations wrapper.
pub struct ECC;

impl ECC {
    /// Generates a public key from private key.
    pub fn generate_public_key(private_key: &[u8; 32], curve: ECCurve) -> CryptoResult<ECPoint> {
        match curve {
            ECCurve::Secp256k1 => {
                let pub_bytes = Secp256k1Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes)
                    .map_err(|e| CryptoError::invalid_point(e.to_string()))
            }
            ECCurve::Secp256r1 => {
                let pub_bytes = Secp256r1Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes)
                    .map_err(|e| CryptoError::invalid_point(e.to_string()))
            }
            ECCurve::Ed25519 => {
                let pub_bytes = Ed25519Crypto::derive_public_key(private_key)?;
                ECPoint::from_bytes_with_curve(curve, &pub_bytes)
                    .map_err(|e| CryptoError::invalid_point(e.to_string()))
            }
        }
    }

    /// Compresses a public key.
    pub fn compress_public_key(public_key: &ECPoint) -> CryptoResult<Vec<u8>> {
        public_key
            .encode_compressed()
            .map_err(|e| CryptoError::invalid_point(e.to_string()))
    }
}

impl Crypto {
    /// Verifies ECDSA signature with secp256r1.
    #[must_use]
    pub fn verify_signature_secp256r1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256r1).unwrap_or(false)
    }

    /// Verifies ECDSA signature with secp256k1.
    #[must_use]
    pub fn verify_signature_secp256k1(data: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        ECDsa::verify(data, signature, public_key, ECCurve::Secp256k1).unwrap_or(false)
    }

    /// Verifies an ECDSA signature using the specified curve and hash algorithm.
    #[must_use]
    pub fn verify_signature_with_curve(
        data: &[u8],
        signature: &[u8],
        public_key: &[u8],
        curve: &ECCurve,
        hash_algorithm: HashAlgorithm,
    ) -> bool {
        if *curve == ECCurve::Ed25519 {
            return ECDsa::verify(data, signature, public_key, *curve).unwrap_or(false);
        }

        if signature.len() != 64 {
            return false;
        }

        let signature: [u8; 64] = match signature.try_into() {
            Ok(signature) => signature,
            Err(_) => return false,
        };

        verify_ecdsa_raw64_with_hash(data, &signature, public_key, *curve, hash_algorithm)
            .unwrap_or(false)
    }

    /// Verifies a signature against the supplied public key, inferring the curve where possible.
    #[must_use]
    pub fn verify_signature_bytes(message: &[u8], signature: &[u8], public_key: &[u8]) -> bool {
        if signature.len() != 64 {
            return false;
        }

        let mut sig = [0u8; 64];
        sig.copy_from_slice(signature);

        match public_key.len() {
            32 => {
                let mut pk = [0u8; 32];
                pk.copy_from_slice(public_key);
                Ed25519Crypto::verify(message, &sig, &pk).unwrap_or(false)
            }
            33 => {
                let mut pk = [0u8; 33];
                pk.copy_from_slice(public_key);
                if Secp256k1Crypto::verify(message, &sig, &pk) == Ok(true) {
                    return true;
                }
                Secp256r1Crypto::verify(message, &sig, public_key).unwrap_or(false)
            }
            64 | 65 => {
                if Secp256r1Crypto::verify(message, &sig, public_key) == Ok(true) {
                    return true;
                }

                if let Ok(pk) = Secp256k1PublicKey::from_slice(public_key) {
                    let compressed = pk.serialize();
                    let mut buf = [0u8; 33];
                    buf.copy_from_slice(&compressed);
                    return Secp256k1Crypto::verify(message, &sig, &buf).unwrap_or(false);
                }
                false
            }
            _ => Secp256r1Crypto::verify(message, &sig, public_key).unwrap_or(false),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{Secp256k1Crypto, Secp256r1Crypto, NEOFS_ECDSA_SHA512_PREFIX};

    #[test]
    fn test_secp256k1_operations() {
        let private_key = Secp256k1Crypto::generate_private_key().unwrap();
        let public_key = Secp256k1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"test message";

        let signature = Secp256k1Crypto::sign(message, &private_key).unwrap();
        let is_valid = Secp256k1Crypto::verify(message, &signature, &public_key).unwrap();

        assert!(is_valid);
    }

    #[test]
    fn neofs_p256_sha512_signs_and_verifies() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"neofs bearer token";

        let signature = Secp256r1Crypto::sign_neofs_sha512(message, &private_key).unwrap();

        assert_eq!(signature.len(), 65);
        assert_eq!(signature[0], NEOFS_ECDSA_SHA512_PREFIX);
        assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature, &public_key).unwrap());
    }

    #[test]
    fn neofs_p256_sha512_rejects_mutated_inputs() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"neofs bearer token";
        let signature = Secp256r1Crypto::sign_neofs_sha512(message, &private_key).unwrap();

        assert!(!Secp256r1Crypto::verify_neofs_sha512(
            b"different message",
            &signature,
            &public_key
        )
        .unwrap());

        let mut mutated = signature;
        mutated[64] ^= 0x01;
        assert!(!Secp256r1Crypto::verify_neofs_sha512(message, &mutated, &public_key).unwrap());

        assert!(
            Secp256r1Crypto::verify_neofs_sha512(message, &signature[..64], &public_key).is_err()
        );
        assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature, &[0x02]).is_err());
    }

    #[test]
    fn neofs_p256_sha512_preserves_ignored_prefix_behavior() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"neofs bearer token";
        let mut signature = Secp256r1Crypto::sign_neofs_sha512(message, &private_key).unwrap();
        signature[0] = 0xff;

        assert!(Secp256r1Crypto::verify_neofs_sha512(message, &signature, &public_key).unwrap());
    }

    #[test]
    fn neofs_p256_sha512_rejects_regular_p256_signature() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let message = b"neofs bearer token";
        let signature = Secp256r1Crypto::sign(message, &private_key).unwrap();
        let mut neofs_shaped_signature = [0u8; 65];
        neofs_shaped_signature[0] = NEOFS_ECDSA_SHA512_PREFIX;
        neofs_shaped_signature[1..].copy_from_slice(&signature);

        assert!(!Secp256r1Crypto::verify_neofs_sha512(
            message,
            &neofs_shaped_signature,
            &public_key
        )
        .unwrap());
    }
}
