//! Witness invocation-script helpers.
//!
//! These helpers encode and decode the canonical single-signature invocation
//! shape used by Neo witnesses: `PUSHDATA1 0x40 <64-byte signature>`.

use crate::OpCode;

use super::ScriptBuilder;

impl ScriptBuilder {
    /// Push a signature onto the stack as a single-sig invocation script.
    ///
    /// For a 64-byte secp256r1 signature this produces the canonical
    /// `PUSHDATA1 0x40 <64-byte sig>` sequence (66 bytes total) that
    /// Neo witness invocation scripts use. The output is byte-identical
    /// to the hand-rolled `PUSHDATA1 + len + sig` construction that was
    /// previously duplicated across neo-consensus and neo-node.
    ///
    /// This is the inverse of [`signature_from_invocation`].
    pub fn invocation_from_signature(&mut self, signature: &[u8]) -> &mut Self {
        self.emit_push(signature)
    }
}

/// Extract the raw signature from a `PUSHDATA1 0x40 <64-byte sig>` invocation
/// script.
///
/// Returns `None` if the script doesn't match this exact shape (wrong length,
/// wrong opcode, or wrong length byte). This is the inverse of
/// [`ScriptBuilder::invocation_from_signature`].
///
/// The returned slice borrows from `script` — no allocation.
pub fn signature_from_invocation(script: &[u8]) -> Option<&[u8]> {
    if script.len() != 66 {
        return None;
    }
    if script[0] != OpCode::PUSHDATA1.byte() || script[1] != 0x40 {
        return None;
    }
    Some(&script[2..66])
}
