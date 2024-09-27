use std::convert::TryInto;
use std::sync::Arc;

use elliptic_curve::{
    sec1::ToEncodedPoint,
    weierstrass::ecdsa::{signature::Verifier, Signature, VerifyingKey},
};
use k256::Secp256k1;
use p256::NistP256;
use sha2::Sha256;
use sha3::Keccak256;

use crate::core::interop::{Context, ContractMD};
use crate::core::native::nativenames;
use crate::crypto::hash::{Hash160, Hash256};
use crate::smartcontract::{manifest, CallFlags};
use crate::types::{StackItem, InteropInterface};
use crate::util::Uint256;

pub struct Crypto {
    contract_md: ContractMD,
}

#[derive(Clone, Copy)]
pub enum NamedCurveHash {
    Secp256k1Sha256 = 22,
    Secp256r1Sha256 = 23,
    Secp256k1Keccak256 = 122,
    Secp256r1Keccak256 = 123,
}

const CRYPTO_CONTRACT_ID: i32 = -3;

impl Crypto {
    pub fn new() -> Self {
        let mut c = Crypto {
            contract_md: ContractMD::new(nativenames::CRYPTO_LIB, CRYPTO_CONTRACT_ID),
        };
        c.build_methods();
        c
    }

    fn build_methods(&mut self) {
        let sha256_desc = manifest::MethodDescriptor::new(
            "sha256",
            vec![manifest::Parameter::new("data", manifest::ParameterType::ByteArray)],
            manifest::ParameterType::ByteArray,
        );
        self.contract_md.add_method(sha256_desc, Self::sha256, 1 << 15, CallFlags::None);

        // Add other methods similarly...
    }

    fn sha256(_ctx: &Context, args: Vec<StackItem>) -> Result<StackItem, String> {
        let data = args[0].try_bytes()?;
        let hash = Hash256::hash(&data);
        Ok(StackItem::ByteArray(hash.to_be_bytes().to_vec()))
    }

    fn ripemd160(_ctx: &Context, args: Vec<StackItem>) -> Result<StackItem, String> {
        let data = args[0].try_bytes()?;
        let hash = Hash160::hash(&data);
        Ok(StackItem::ByteArray(hash.to_be_bytes().to_vec()))
    }

    fn murmur32(_ctx: &Context, args: Vec<StackItem>) -> Result<StackItem, String> {
        let data = args[0].try_bytes()?;
        let seed = args[1].try_integer()?.try_into().map_err(|_| "Invalid seed")?;
        let hash = murmur3::murmur3_32(&mut rand::thread_rng(), &data, seed);
        Ok(StackItem::ByteArray(hash.to_le_bytes().to_vec()))
    }

    fn verify_with_ecdsa(_ctx: &Context, args: Vec<StackItem>) -> Result<StackItem, String> {
        verify_with_ecdsa_generic(args, true)
    }

    // Implement other methods...
}

fn verify_with_ecdsa_generic(args: Vec<StackItem>, allow_keccak: bool) -> Result<StackItem, String> {
    let msg = args[0].try_bytes()?;
    let pubkey = args[1].try_bytes()?;
    let signature = args[2].try_bytes()?;
    let (curve, hasher) = curve_hasher_from_stackitem(&args[3], allow_keccak)?;

    let hash_to_check = hasher(&msg);
    let pkey = decode_public_key(&pubkey, curve)?;
    let sig = Signature::from_bytes(&signature).map_err(|e| e.to_string())?;

    let res = pkey.verify(&hash_to_check, &sig).is_ok();
    Ok(StackItem::Boolean(res))
}

fn curve_hasher_from_stackitem(item: &StackItem, allow_keccak: bool) -> Result<(Arc<dyn elliptic_curve::Curve>, Box<dyn Fn(&[u8]) -> [u8; 32]>), String> {
    let curve_hash = item.try_integer()?.try_into().map_err(|_| "Invalid curve hash")?;
    match NamedCurveHash::from(curve_hash) {
        NamedCurveHash::Secp256k1Sha256 => Ok((Arc::new(Secp256k1), Box::new(|data| Sha256::digest(data).into()))),
        NamedCurveHash::Secp256r1Sha256 => Ok((Arc::new(NistP256), Box::new(|data| Sha256::digest(data).into()))),
        NamedCurveHash::Secp256k1Keccak256 | NamedCurveHash::Secp256r1Keccak256 if allow_keccak => {
            let curve = if curve_hash == NamedCurveHash::Secp256k1Keccak256 as i64 {
                Arc::new(Secp256k1)
            } else {
                Arc::new(NistP256)
            };
            Ok((curve, Box::new(|data| Keccak256::digest(data).into())))
        }
        _ => Err("Unsupported curve/hash combination".into()),
    }
}

fn decode_public_key(pubkey: &[u8], curve: Arc<dyn elliptic_curve::Curve>) -> Result<VerifyingKey<impl elliptic_curve::Curve>, String> {
    VerifyingKey::from_sec1_bytes(pubkey).map_err(|e| format!("Failed to decode pubkey: {}", e))
}

// Implement other helper functions...
