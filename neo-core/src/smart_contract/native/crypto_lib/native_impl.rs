use super::*;
use crate::smart_contract::native::NativeContract;
use std::any::Any;

impl NativeContract for CryptoLib {
    fn id(&self) -> i32 {
        self.id
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "CryptoLib"
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        match method {
            "recoverSecp256K1" => self.recover_secp256k1(args),
            "sha256" => self.sha256(args),
            "ripemd160" => self.ripemd160(args),
            "murmur32" => self.murmur32(args),
            "keccak256" => self.keccak256(args),
            "verifyWithECDsa" => self.verify_with_ecdsa(engine, args),
            "verifyWithEd25519" => self.verify_with_ed25519(args),
            "bls12381Add" => self.bls12381_add(args),
            "bls12381Equal" => self.bls12381_equal(args),
            "bls12381Mul" => self.bls12381_mul(args),
            "bls12381Pairing" => self.bls12381_pairing(args),
            "bls12381Serialize" => self.bls12381_serialize(args),
            "bls12381Deserialize" => self.bls12381_deserialize(args),
            _ => Err(Error::native_contract(format!(
                "Unknown CryptoLib method: {}",
                method
            ))),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for CryptoLib {
    fn default() -> Self {
        Self::new()
    }
}
