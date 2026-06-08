//! Shared wallet witness-script helpers.
//!
//! These build the invocation/verification scripts a wallet emits when signing,
//! and belong with the wallet layer (C# `Neo.Wallets`) rather than in the node
//! daemon, so any wallet implementation (software, HSM, TEE) can reuse them.

use crate::{WalletError, WalletResult};
use neo_script_builder::ScriptBuilder;

/// Builds the witness invocation script for a single 64-byte signature
/// (`PUSHDATA1 0x40 <signature>`), matching the C# wallet signing path.
pub fn signature_invocation(signature: &[u8]) -> WalletResult<Vec<u8>> {
    if signature.len() != 64 {
        return Err(WalletError::SigningFailed(
            "Signature must be 64 bytes".to_string(),
        ));
    }

    let mut builder = ScriptBuilder::new();
    builder.emit_push(signature);
    Ok(builder.to_array())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_vm_rs::OpCode;

    #[test]
    fn signature_invocation_uses_neo_pushdata1_form() {
        let signature = [0xAB; 64];
        let invocation = signature_invocation(&signature).expect("invocation script");

        assert_eq!(invocation.len(), 66);
        assert_eq!(invocation[0], OpCode::PUSHDATA1.byte());
        assert_eq!(invocation[1], 0x40);
        assert_eq!(&invocation[2..], signature);
    }

    #[test]
    fn signature_invocation_rejects_non_64_byte_signature() {
        assert!(signature_invocation(&[0xAB; 63]).is_err());
        assert!(signature_invocation(&[0xAB; 65]).is_err());
    }
}
