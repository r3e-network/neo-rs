//! dBFT extensible-payload codec.
//!
//! Neo dBFT messages travel over Neo's `ExtensiblePayload` wire envelope. This
//! module keeps the category, sender validation, and witness signature
//! extraction together so the consensus driver only routes already-decoded
//! consensus messages.

use neo_consensus::ValidatorInfo;
use neo_consensus::messages::ConsensusPayload;
use neo_payloads::{ExtensiblePayload, Witness};
use neo_vm::script_builder::{RedeemScript, ScriptBuilder, signature_from_invocation};

/// dBFT extensible category (C# `ConsensusContext.CreatePayload`: `Category = "dBFT"`).
pub(super) const DBFT_CATEGORY: &str = "dBFT";

/// Builds the outbound dBFT [`ExtensiblePayload`] for a `ConsensusPayload` the
/// service produced (its `witness` is the raw 64-byte signature). Mirrors C#
/// `ConsensusContext.CreatePayload`.
pub(super) fn consensus_to_extensible(
    payload: &ConsensusPayload,
    validators: &[ValidatorInfo],
) -> Option<ExtensiblePayload> {
    let validator = validators.get(payload.validator_index as usize)?;
    let mut ext = ExtensiblePayload::new();
    ext.category = DBFT_CATEGORY.to_string();
    ext.valid_block_start = 0;
    ext.valid_block_end = payload.block_index;
    ext.sender = validator.script_hash;
    ext.data = payload.to_message_bytes();
    ext.witness = Witness::new_with_scripts(
        ScriptBuilder::new()
            .invocation_from_signature(&payload.witness)
            .to_array(),
        RedeemScript::signature_redeem_script(&validator.public_key.encoded()),
    );
    Some(ext)
}

/// Decodes an inbound dBFT [`ExtensiblePayload`] into a [`ConsensusPayload`].
/// Returns `None` for non-dBFT, malformed, or spoofed payloads (the in-body
/// `validator_index` must map to the validator whose script hash is the
/// extensible's `sender`).
pub fn extensible_to_consensus(
    ext: &ExtensiblePayload,
    network: u32,
    validators: &[ValidatorInfo],
) -> Option<ConsensusPayload> {
    if ext.category != DBFT_CATEGORY {
        return None;
    }
    let signature = signature_from_invocation(&ext.witness.invocation_script)?;
    let payload =
        ConsensusPayload::from_message_bytes(network, &ext.data, signature.to_vec()).ok()?;
    let validator = validators.get(payload.validator_index as usize)?;
    if validator.script_hash != ext.sender {
        return None;
    }
    Some(payload)
}
