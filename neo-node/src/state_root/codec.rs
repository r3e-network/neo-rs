//! StateService extensible-payload codec helpers.
//!
//! The codec here only wraps and unwraps Neo `ExtensiblePayload` envelopes for
//! StateService votes and signed roots. Vote aggregation, verification, and
//! persistence stay in the driver and blockchain/state-service layers.

use neo_crypto::{ECPoint, Secp256r1Crypto};
use neo_payloads::{ExtensiblePayload, Witness};
use neo_primitives::UInt160;
use neo_state_service::{MessageType, STATE_SERVICE_CATEGORY};
use neo_vm::script_builder::{RedeemScript, ScriptBuilder};

/// Vote extensible `ValidBlockEnd` reach past the root index (C#
/// `VerificationService.MaxCachedVerificationProcessCount`).
pub(super) const VOTE_VALID_BLOCK_END_THRESHOLD: u32 = 10;
/// StateRoot extensible `ValidBlockEnd` reach past the root index (C#
/// `VerificationContext.MaxValidUntilBlockIncrement`).
pub(super) const STATE_ROOT_VALID_BLOCK_END_THRESHOLD: u32 = 100;

/// Builds a `StateService` extensible carrying a `[MessageType][payload]` body,
/// signed by the sender's key. Mirrors C# `VerificationContext.CreatePayload`.
pub(super) fn build_extensible(
    message_type: MessageType,
    payload_bytes: &[u8],
    root_index: u32,
    valid_block_end_threshold: u32,
    private_key: &[u8; 32],
    public_key: &ECPoint,
    network: u32,
) -> Option<ExtensiblePayload> {
    let mut data = Vec::with_capacity(1 + payload_bytes.len());
    data.push(message_type.to_byte());
    data.extend_from_slice(payload_bytes);

    let redeem = RedeemScript::signature_redeem_script(public_key.as_bytes());
    let mut ext = ExtensiblePayload::new();
    ext.category = STATE_SERVICE_CATEGORY.to_string();
    ext.valid_block_start = root_index;
    ext.valid_block_end = root_index.saturating_add(valid_block_end_threshold);
    ext.sender = UInt160::from_script(&redeem);
    ext.data = data;

    // Sign the extensible itself so peers accept and relay it (its witness must
    // match `sender`). Sign-data = network magic (LE) || payload hash.
    let hash = ext.hash();
    let mut sign_data = [0u8; 4 + 32];
    sign_data[..4].copy_from_slice(&network.to_le_bytes());
    sign_data[4..].copy_from_slice(&hash.to_bytes());
    let signature = Secp256r1Crypto::sign(&sign_data, private_key).ok()?;
    ext.witness = Witness::new_with_scripts(
        ScriptBuilder::new()
            .invocation_from_signature(&signature)
            .to_array(),
        redeem,
    );
    Some(ext)
}

/// Splits an inbound `StateService` extensible into its `(MessageType, body)`.
pub(super) fn decode_message(ext: &ExtensiblePayload) -> Option<(MessageType, &[u8])> {
    if ext.category != STATE_SERVICE_CATEGORY {
        return None;
    }
    let (&type_byte, body) = ext.data.split_first()?;
    let message_type = MessageType::from_byte(type_byte)?;
    Some((message_type, body))
}
