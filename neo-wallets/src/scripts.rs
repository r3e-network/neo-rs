//! Shared wallet witness-script helpers.
//!
//! These build the invocation/verification scripts a wallet emits when signing,
//! and belong with the wallet layer (C# `Neo.Wallets`) rather than in the node
//! daemon, so any wallet implementation (software, HSM, TEE) can reuse them.

use crate::{WalletError, WalletResult};
use neo_vm::script_builder::ScriptBuilder;

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
#[path = "tests/scripts.rs"]
mod tests;
