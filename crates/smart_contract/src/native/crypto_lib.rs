use crate::application_engine::ApplicationEngine;
use crate::native::{NativeContract, NativeMethod};
use crate::Result;
use neo_core::UInt160;
pub struct CryptoLib {
    hash: UInt160,
    methods: Vec<NativeMethod>,
}
impl CryptoLib {
    pub fn new() -> Self {
        let hash = UInt160::from_bytes(&[
            0x72, 0x6c, 0xb6, 0xe0, 0xcd, 0x8c, 0x99, 0x83, 0x91, 0x78, 0xee, 0xc0, 0x85, 0xfd,
            0x4f, 0x2e, 0x4b, 0xaf, 0x01, 0x25,
        ])
        .expect("Valid CryptoLib contract hash");
        let methods = vec![NativeMethod::safe("bls12381Add".to_string(), 1 << 19)];
        Self { hash, methods }
    }
}
impl NativeContract for CryptoLib {
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
        _engine: &mut ApplicationEngine,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        Ok(vec![])
    }
}
