use alloc::rc::Rc;
use std::collections::HashMap;
use lazy_static::lazy_static;
use neo_proc_macros::{contract, contract_impl};
use crate::cryptography::{ECCurve, Hasher};
use crate::neo_contract::native_contract::named_curve_hash::NamedCurveHash;

/// A native contract library that provides cryptographic algorithms.
#[contract]
pub struct CryptoLib;

lazy_static! {
    static ref CURVES: HashMap<NamedCurveHash, (Rc<ECCurve>, Hasher)> = {
        let mut m = HashMap::new();
        m.insert(NamedCurveHash::Secp256k1SHA256, (ECCurve::secp256k1(), Hasher::SHA256));
        m.insert(NamedCurveHash::Secp256r1SHA256, (ECCurve::secp256r1(), Hasher::SHA256));
        m.insert(NamedCurveHash::Secp256k1Keccak256, (ECCurve::secp256k1(), Hasher::Keccak256));
        m.insert(NamedCurveHash::Secp256r1Keccak256, (ECCurve::secp256r1(), Hasher::Keccak256));
        m
    };
}

#[contract_impl]
impl CryptoLib {
    /// Computes the hash value for the specified byte array using the ripemd160 algorithm.
    #[contract_method(cpu_fee = 1 << 15, name = "ripemd160")]
    pub fn ripemd160(data: &[u8]) -> Vec<u8> {
        ripemd160(data)
    }

    /// Computes the hash value for the specified byte array using the sha256 algorithm.
    #[contract_method(cpu_fee = 1 << 15)]
    pub fn sha256(data: &[u8]) -> Vec<u8> {
        sha256(data)
    }

    /// Computes the hash value for the specified byte array using the murmur32 algorithm.
    #[contract_method(cpu_fee = 1 << 13)]
    pub fn murmur32(data: &[u8], seed: u32) -> Vec<u8> {
        murmur32(data, seed)
    }

    /// Computes the hash value for the specified byte array using the keccak256 algorithm.
    #[contract_method(cpu_fee = 1 << 15, active_in = Hardfork::HF_Cockatrice)]
    pub fn keccak256(data: &[u8]) -> Vec<u8> {
        keccak256(data)
    }

    /// Verifies that a digital signature is appropriate for the provided key and message using the ECDSA algorithm.
    #[contract_method(cpu_fee = 1 << 15, active_in = Hardfork::HF_Cockatrice)]
    pub fn verify_with_ecdsa(message: &[u8], pubkey: &[u8], signature: &[u8], curve_hash: NamedCurveHash) -> bool {
        match CURVES.get(&curve_hash) {
            Some((curve, hasher)) => {
                crypto::verify_signature(message, signature, pubkey, curve, hasher)
            },
            None => false,
        }
    }

    // This is for solving the hardfork issue in https://github.com/neo-project/neo/pull/3209
    #[contract_method(cpu_fee = 1 << 15, name = "verifyWithECDsa", active_in = Hardfork::HF_Cockatrice)]
    pub fn verify_with_ecdsa_v0(message: &[u8], pubkey: &[u8], signature: &[u8], curve: NamedCurveHash) -> Result<bool, String> {
        if curve != NamedCurveHash::Secp256k1SHA256 && curve != NamedCurveHash::Secp256r1SHA256 {
            return Err("Invalid curve".into());
        }

        match CURVES.get(&curve) {
            Some((curve, _)) => {
                Ok(crypto::verify_signature(message, signature, pubkey, curve))
            },
            None => Ok(false),
        }
    }
}
