//! Property-based tests (proptest) over real neo-rs consensus code.
//!
//! These exercise the *actual* on-wire serialization of `ConsensusPayload` —
//! `to_message_bytes` / `from_message_bytes` — across thousands of random
//! inputs, asserting the wire format invariants and a full round-trip.

use neo_consensus::{ConsensusMessageType, messages::ConsensusPayload};
use proptest::prelude::*;

/// Strategy that always yields one of the six real dBFT message types.
fn msg_type_strategy() -> impl Strategy<Value = ConsensusMessageType> {
    prop_oneof![
        Just(ConsensusMessageType::ChangeView),
        Just(ConsensusMessageType::PrepareRequest),
        Just(ConsensusMessageType::PrepareResponse),
        Just(ConsensusMessageType::Commit),
        Just(ConsensusMessageType::RecoveryRequest),
        Just(ConsensusMessageType::RecoveryMessage),
    ]
}

proptest! {
    /// Round-trip: serializing then parsing a payload must recover every field,
    /// and the bytes must match the DBFTPlugin layout
    /// `[type:1][block_index:4][validator_index:1][view_number:1][data...]`.
    #[test]
    fn consensus_payload_round_trip(
        network in any::<u32>(),
        block_index in any::<u32>(),
        validator_index in any::<u8>(),
        view_number in any::<u8>(),
        msg_type in msg_type_strategy(),
        data in any::<Vec<u8>>(),
    ) {
        let payload = ConsensusPayload::new(
            network,
            block_index,
            validator_index,
            view_number,
            msg_type,
            data.clone(),
        );
        let bytes = payload.to_message_bytes();

        // Wire-format invariants.
        prop_assert_eq!(bytes.len(), 7 + data.len(), "payload body length");
        prop_assert_eq!(bytes[0], msg_type.to_byte(), "type byte");
        prop_assert_eq!(&bytes[1..5], &block_index.to_le_bytes(), "block_index LE");
        prop_assert_eq!(bytes[5], validator_index, "validator_index");
        prop_assert_eq!(bytes[6], view_number, "view_number");
        prop_assert_eq!(&bytes[7..], &data[..], "body");

        // Round-trip.
        let parsed =
            ConsensusPayload::from_message_bytes(network, &bytes, Vec::new()).unwrap();
        prop_assert_eq!(parsed.block_index, block_index);
        prop_assert_eq!(parsed.validator_index, validator_index);
        prop_assert_eq!(parsed.view_number, view_number);
        prop_assert_eq!(parsed.message_type, msg_type);
        prop_assert_eq!(parsed.data, data);
    }

    /// Changing any byte of the body must change the parsed payload's data
    /// (serialization is injective over `data` for a fixed type header).
    #[test]
    fn body_is_injective(
        network in any::<u32>(),
        block_index in any::<u32>(),
        validator_index in any::<u8>(),
        view_number in any::<u8>(),
        msg_type in msg_type_strategy(),
        base in any::<Vec<u8>>(),
        index in 0usize..64,
        byte in 0u8..255,
    ) {
        // Only consider non-empty bodies where flipping a byte actually changes it.
        if base.is_empty() {
            return Ok(());
        }
        let idx = index % base.len();
        let mut modified = base.clone();
        modified[idx] ^= byte;
        if modified == base {
            return Ok(());
        }

        let a = ConsensusPayload::new(
            network, block_index, validator_index, view_number, msg_type, base.clone(),
        );
        let b = ConsensusPayload::new(
            network, block_index, validator_index, view_number, msg_type, modified.clone(),
        );
        let pa = ConsensusPayload::from_message_bytes(
            network, &a.to_message_bytes(), Vec::new()).unwrap();
        let pb = ConsensusPayload::from_message_bytes(
            network, &b.to_message_bytes(), Vec::new()).unwrap();
        prop_assert_ne!(pa.data, pb.data, "data must differ when a body byte differs");
    }
}

/// Ties to `formal/coqlib/wire_format_refinement.v`: the header is a constant
/// 7 bytes, so total wire length = 7 + body length.
#[test]
fn wire_format_length_invariants() {
    let network = 0x4E45_4F00u32;
    for data_len in [0usize, 1, 32, 256] {
        let payload = ConsensusPayload::new(
            network,
            0,
            0,
            0,
            ConsensusMessageType::Commit,
            vec![0u8; data_len],
        );
        assert_eq!(
            payload.to_message_bytes().len(),
            7 + data_len,
            "wire length for body {data_len}"
        );
    }
}
