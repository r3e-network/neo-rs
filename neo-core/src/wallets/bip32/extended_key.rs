//! BIP-32 extended key derivation.

use super::key_path::KeyPath;
use crate::cryptography::{ECCurve, ECPoint, ECC};
use hmac::{Hmac, Mac};
use num_bigint::BigUint;
use num_traits::Zero;
use once_cell::sync::Lazy;
use p256::elliptic_curve::bigint::ArrayEncoding;
use p256::elliptic_curve::Curve;
use sha2::Sha512;

type HmacSha512 = Hmac<Sha512>;

static SECP256R1_ORDER: Lazy<BigUint> =
    Lazy::new(|| BigUint::from_bytes_be(&p256::NistP256::ORDER.to_be_byte_array()));

static SECP256K1_ORDER: Lazy<BigUint> =
    Lazy::new(|| BigUint::from_bytes_be(&k256::Secp256k1::ORDER.to_be_byte_array()));

#[derive(Clone, Debug)]
pub struct ExtendedKey {
    pub private_key: [u8; 32],
    pub public_key: ECPoint,
    pub chain_code: [u8; 32],
}

impl ExtendedKey {
    pub fn create(seed: &[u8], curve: Option<ECCurve>) -> Result<Self, String> {
        let curve = curve.unwrap_or(ECCurve::Secp256r1);
        if matches!(curve, ECCurve::Ed25519) {
            return Err("Ed25519 is not supported for BIP32".to_string());
        }
        let i = hmac_sha512(b"Bitcoin seed", seed)?;
        let mut private_key = [0u8; 32];
        let mut chain_code = [0u8; 32];
        private_key.copy_from_slice(&i[..32]);
        chain_code.copy_from_slice(&i[32..]);

        let public_key = ECC::generate_public_key(&private_key, curve)?;

        Ok(Self {
            private_key,
            public_key,
            chain_code,
        })
    }

    pub fn create_with_path(
        seed: &[u8],
        path: &str,
        curve: Option<ECCurve>,
    ) -> Result<Self, String> {
        let key_path = KeyPath::parse(path)?;
        let mut ext_key = Self::create(seed, curve)?;
        for index in key_path.indices() {
            ext_key = ext_key.derive(*index)?;
        }
        Ok(ext_key)
    }

    pub fn derive(&self, index: u32) -> Result<Self, String> {
        let mut data = [0u8; 37];
        if index >= 0x8000_0000 {
            data[0] = 0;
            data[1..33].copy_from_slice(&self.private_key);
        } else {
            let pub_bytes = self.public_key.encode_point(true)?;
            if pub_bytes.len() != 33 {
                return Err("Invalid public key length".to_string());
            }
            data[..33].copy_from_slice(&pub_bytes);
        }
        data[33..].copy_from_slice(&index.to_be_bytes());

        let i = hmac_sha512(&self.chain_code, &data)?;
        let il = &i[..32];
        let mut chain_code = [0u8; 32];
        chain_code.copy_from_slice(&i[32..]);

        let order = curve_order(self.public_key.curve())?;
        let private_key = add_mod_n(il, &self.private_key, &order)?;
        let public_key = ECC::generate_public_key(&private_key, self.public_key.curve())?;

        Ok(Self {
            private_key,
            public_key,
            chain_code,
        })
    }
}

fn hmac_sha512(key: &[u8], data: &[u8]) -> Result<[u8; 64], String> {
    let mut mac =
        HmacSha512::new_from_slice(key).map_err(|_| "Invalid HMAC key length".to_string())?;
    mac.update(data);
    let result = mac.finalize().into_bytes();
    let mut out = [0u8; 64];
    out.copy_from_slice(&result);
    Ok(out)
}

fn curve_order(curve: ECCurve) -> Result<BigUint, String> {
    match curve {
        ECCurve::Secp256r1 => Ok(SECP256R1_ORDER.clone()),
        ECCurve::Secp256k1 => Ok(SECP256K1_ORDER.clone()),
        ECCurve::Ed25519 => Err("Ed25519 is not supported for BIP32".to_string()),
    }
}

fn add_mod_n(a: &[u8], b: &[u8; 32], n: &BigUint) -> Result<[u8; 32], String> {
    let a_int = BigUint::from_bytes_be(a);
    if a_int >= *n {
        return Err("Derived child private key is invalid.".to_string());
    }

    let b_int = BigUint::from_bytes_be(b);
    let r = (a_int + b_int) % n;
    if r.is_zero() {
        return Err("Derived child private key is invalid.".to_string());
    }

    let mut result = [0u8; 32];
    let r_bytes = r.to_bytes_be();
    result[32 - r_bytes.len()..].copy_from_slice(&r_bytes);
    Ok(result)
}
