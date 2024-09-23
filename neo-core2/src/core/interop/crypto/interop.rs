use crate::core::interop::{self, interopnames};
use crate::core::interop::crypto::{ECDSASecp256r1CheckMultisig, ECDSASecp256r1CheckSig};

lazy_static::lazy_static! {
    static ref NEO_CRYPTO_CHECK_MULTISIG_ID: u32 = interopnames::to_id(interopnames::SYSTEM_CRYPTO_CHECK_MULTISIG.as_bytes());
    static ref NEO_CRYPTO_CHECK_SIG_ID: u32 = interopnames::to_id(interopnames::SYSTEM_CRYPTO_CHECK_SIG.as_bytes());
}

// Interops represents sorted crypto-related interop functions.
pub static mut INTEROPS: Vec<interop::Function> = Vec::new();

pub fn init() {
    unsafe {
        INTEROPS.push(interop::Function { id: *NEO_CRYPTO_CHECK_MULTISIG_ID, func: ECDSASecp256r1CheckMultisig });
        INTEROPS.push(interop::Function { id: *NEO_CRYPTO_CHECK_SIG_ID, func: ECDSASecp256r1CheckSig });
        INTEROPS.sort_by(|a, b| a.id.cmp(&b.id));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_init() {
        init();
        unsafe {
            assert_eq!(INTEROPS.len(), 2);
            assert_eq!(INTEROPS[0].id, *NEO_CRYPTO_CHECK_MULTISIG_ID);
            assert_eq!(INTEROPS[1].id, *NEO_CRYPTO_CHECK_SIG_ID);
        }
    }
}
