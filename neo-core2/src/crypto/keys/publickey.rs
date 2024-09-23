use ecdsa::{elliptic_curve::sec1::ToEncodedPoint, signature::Verifier, VerifyingKey};
use elliptic_curve::bigint::U256;
use k256::Secp256k1;
use p256::NistP256;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;
use std::sync::Mutex;
use lru::LruCache;
use hex;
use std::str::FromStr;

const COORD_LEN: usize = 32;
const SIGNATURE_LEN: usize = 64;

lazy_static! {
    static ref KEY_CACHE: Mutex<LruCache<String, PublicKey>> = Mutex::new(LruCache::new(1024));
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PublicKey {
    x: U256,
    y: U256,
    curve: String,
}

impl PublicKey {
    pub fn new_from_string(s: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let bytes = hex::decode(s)?;
        Self::new_from_bytes(&bytes, "P256")
    }

    pub fn new_from_bytes(bytes: &[u8], curve: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let mut cache = KEY_CACHE.lock().unwrap();
        if let Some(pub_key) = cache.get(&hex::encode(bytes)) {
            return Ok(pub_key.clone());
        }

        let (x, y) = match curve {
            "P256" => {
                let point = NistP256::from_encoded_point(&bytes).ok_or("Invalid point")?;
                (point.x().unwrap().clone(), point.y().unwrap().clone())
            }
            "Secp256k1" => {
                let point = Secp256k1::from_encoded_point(&bytes).ok_or("Invalid point")?;
                (point.x().unwrap().clone(), point.y().unwrap().clone())
            }
            _ => return Err("Unsupported curve".into()),
        };

        let pub_key = PublicKey {
            x,
            y,
            curve: curve.to_string(),
        };

        cache.put(hex::encode(bytes), pub_key.clone());
        Ok(pub_key)
    }

    pub fn bytes(&self, compressed: bool) -> Vec<u8> {
        match self.curve.as_str() {
            "P256" => {
                let point = NistP256::from_affine_coordinates(&self.x, &self.y, compressed);
                point.to_encoded_point(compressed).as_bytes().to_vec()
            }
            "Secp256k1" => {
                let point = Secp256k1::from_affine_coordinates(&self.x, &self.y, compressed);
                point.to_encoded_point(compressed).as_bytes().to_vec()
            }
            _ => vec![],
        }
    }

    pub fn verify(&self, signature: &[u8], hash: &[u8]) -> bool {
        if signature.len() != SIGNATURE_LEN {
            return false;
        }

        let r = U256::from_be_bytes(&signature[0..32]);
        let s = U256::from_be_bytes(&signature[32..64]);

        match self.curve.as_str() {
            "P256" => {
                let verifying_key = VerifyingKey::from_affine_coordinates(&self.x, &self.y, false);
                verifying_key.verify(hash, &(r, s)).is_ok()
            }
            "Secp256k1" => {
                let verifying_key = VerifyingKey::from_affine_coordinates(&self.x, &self.y, false);
                verifying_key.verify(hash, &(r, s)).is_ok()
            }
            _ => false,
        }
    }
}

impl fmt::Display for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.x.is_zero() && self.y.is_zero() {
            write!(f, "00")
        } else {
            write!(f, "{}{}", hex::encode(self.x.to_bytes()), hex::encode(self.y.to_bytes()))
        }
    }
}

impl PublicKey {
    pub fn from_asn1(data: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let pubkey = x509::parse_pkix_public_key(data)?;
        let pk = pubkey.as_ref().downcast_ref::<ecdsa::VerifyingKey>().ok_or("Invalid key")?;
        Ok(PublicKey {
            x: pk.x().clone(),
            y: pk.y().clone(),
            curve: "P256".to_string(),
        })
    }
}
