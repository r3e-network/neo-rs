use std::sync::Arc;
use std::collections::HashMap;
use std::sync::Mutex;
use lazy_static::lazy_static;
use sha2::{Sha256, Digest};

// Assuming ecc and Hasher are defined elsewhere in the codebase
use crate::cryptography::{ecc, ECPoint, Hasher};

lazy_static! {
    static ref CACHE_ECDSA: Mutex<HashMap<ECPoint, Arc<VerifyingKey>>> = Mutex::new(HashMap::new());
}

/// A cryptographic helper struct.
pub struct Crypto;

impl Crypto {
    /// Calculates the 160-bit hash value of the specified message.
    pub fn hash160(message: &[u8]) -> Vec<u8> {
        let sha256 = Sha256::digest(message);
        Ripemd160::digest(&sha256).to_vec()
    }

    /// Calculates the 256-bit hash value of the specified message.
    pub fn hash256(message: &[u8]) -> Vec<u8> {
        let sha256 = Sha256::digest(message);
        Sha256::digest(&sha256).to_vec()
    }

    /// Signs the specified message using the ECDSA algorithm and specified hash algorithm.
    pub fn sign(message: &[u8], pri_key: &[u8], ec_curve: Option<ecc::ECCurve>, hasher: Hasher) -> Vec<u8> {
        let secret_key = SecretKey::from_bytes(pri_key).expect("Invalid private key");
        let signing_key = SigningKey::from(&secret_key);

        let message_hash = match hasher {
            Hasher::SHA256 => Sha256::digest(message).to_vec(),
            Hasher::Keccak256 => todo!("Implement Keccak256"),
            _ => panic!("Unsupported hasher"),
        };

        let signature: Signature = signing_key.sign(&message_hash);
        signature.to_vec()
    }

    /// Verifies that a digital signature is appropriate for the provided key, message and hash algorithm.
    pub fn verify_signature(message: &[u8], signature: &[u8], pubkey: &ecc::ECPoint, hasher: Hasher) -> bool {
        if signature.len() != 64 {
            return false;
        }

        let verifying_key = Self::create_ecdsa(pubkey);

        let message_hash = match hasher {
            Hasher::SHA256 => Sha256::digest(message).to_vec(),
            Hasher::Keccak256 => todo!("Implement Keccak256"),
            _ => panic!("Unsupported hasher"),
        };

        verifying_key.verify(&message_hash, &Signature::from_bytes(signature).unwrap()).is_ok()
    }

    /// Create and cache ECDsa objects
    fn create_ecdsa(pubkey: &ecc::ECPoint) -> Arc<VerifyingKey> {
        let mut cache = CACHE_ECDSA.lock().unwrap();
        if let Some(cached) = cache.get(pubkey) {
            return Arc::clone(cached);
        }

        let encoded_point = pubkey.encode_point(false);
        let verifying_key = VerifyingKey::from_sec1_bytes(&encoded_point).expect("Invalid public key");
        let arc_key = Arc::new(verifying_key);
        cache.insert(pubkey.clone(), Arc::clone(&arc_key));
        arc_key
    }

    /// Verifies that a digital signature is appropriate for the provided key, curve, message and hasher.
    pub fn verify_signature_with_curve(message: &[u8], signature: &[u8], pubkey: &[u8], curve: ecc::ECCurve, hasher: Hasher) -> bool {
        let ec_point = ecc::ECPoint::decode_point(pubkey, curve);
        Self::verify_signature(message, signature, &ec_point, hasher)
    }
}
