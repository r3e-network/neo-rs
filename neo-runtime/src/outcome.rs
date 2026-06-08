//! Placeholder outcome / payload types shared by the reth-style services.
//!
//! The execution, validation, and network-event payloads in the runtime
//! service traits are deliberately *minimal* in this stage. The full
//! per-block execution result (logs, notifications, GAS consumed, state
//! root) will be sourced from `neo-execution` once those concrete
//! implementations land in later stages. Defining the placeholder types
//! here keeps the service traits stable while the real data flows in:
//! the trait signatures will not change, only the bodies of the
//! `ExecutionOutcome` / `ExecutionPayload` / `ValidationResult` structs.
//!
//! `NetworkEvent` is a sealed-style sum type covering the events a
//! `NetworkService` is expected to broadcast on its `broadcast::Sender`.
//! Concrete `neo-network` work in a later stage will replace the inner
//! fields with the canonical network-event types from `neo-wire` /
//! `neo-payloads`.

use neo_primitives::UInt256;
use serde::{Deserialize, Serialize};

/// Result of executing a block via the [`crate::NeoEngine`] /
/// [`crate::BlockExecutor`] services.
///
/// In the steady state this will be sourced from `neo-execution` and carry
/// the per-block GAS consumed, state-root delta, and the emitted
/// `ApplicationExecuted` log. For Stage A it is a struct holding the block
/// hash and a placeholder `ok` flag so the service trait signatures are
/// usable end-to-end.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionOutcome {
    /// Hash of the block the outcome corresponds to.
    pub block_hash: UInt256,
    /// Height of the block the outcome corresponds to.
    pub block_height: u32,
    /// `true` when the block executed without a VM fault.
    pub ok: bool,
    /// Total GAS consumed by the block (placeholder; populated by the
    /// concrete engine implementation in a later stage).
    pub gas_consumed: u64,
}

impl ExecutionOutcome {
    /// Convenience constructor for a successful outcome.
    pub fn success(block_hash: UInt256, block_height: u32, gas_consumed: u64) -> Self {
        Self {
            block_hash,
            block_height,
            ok: true,
            gas_consumed,
        }
    }

    /// Convenience constructor for a failed outcome.
    pub fn failure(block_hash: UInt256, block_height: u32) -> Self {
        Self {
            block_hash,
            block_height,
            ok: false,
            gas_consumed: 0,
        }
    }
}

/// Engine-API-style execution payload.
///
/// In reth this is the typed return value of `engine_executePayload`; in
/// neo-rs it is the analogous blob returned by [`crate::NeoEngine::execute_block`].
/// For Stage A the payload is just an `ExecutionOutcome`; future stages
/// will extend it with the raw execution notifications and the post-state
/// root.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionPayload {
    /// The execution outcome for the block.
    pub outcome: ExecutionOutcome,
}

/// Result of validating (but not executing) a block.
///
/// The validation pipeline runs in two phases in the reth model: a cheap
/// *consensus* check (header / merkle / witness shape) and an expensive
/// *execution* check (state transition). [`crate::NeoEngine::validate_block`]
/// is the cheap, consensus-shaped variant — anything beyond header / merkle
/// belongs in `execute_block`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidationResult {
    /// `true` when every check the validator is responsible for passed.
    pub valid: bool,
    /// Optional human-readable reason for a `valid == false` result.
    pub reason: Option<String>,
}

impl ValidationResult {
    /// Construct a `valid == true` result.
    pub fn ok() -> Self {
        Self {
            valid: true,
            reason: None,
        }
    }

    /// Construct a `valid == false` result with a reason.
    pub fn invalid<E: ToString>(reason: E) -> Self {
        Self {
            valid: false,
            reason: Some(reason.to_string()),
        }
    }
}

/// Events broadcast by a [`crate::NetworkService`] on its
/// `tokio::sync::broadcast::Sender`.
///
/// The variant set mirrors the small set of "lifecycle" events a
/// network service is expected to publish: peer changes, blocks
/// received, transactions received. Future stages will swap the inner
/// types for the canonical `neo-wire` / `neo-payloads` representations.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkEvent {
    /// A new peer has joined the connected set.
    PeerConnected {
        /// Stable identifier for the peer (currently a placeholder string
        /// until `neo-wire` defines a peer-id type).
        peer_id: String,
    },
    /// A previously connected peer has dropped.
    PeerDisconnected {
        /// Stable identifier for the peer.
        peer_id: String,
    },
    /// A block was received from a remote peer.
    BlockReceived {
        /// Hash of the received block.
        block_hash: UInt256,
    },
    /// A transaction was received from a remote peer.
    TransactionReceived {
        /// Hash of the received transaction.
        tx_hash: UInt256,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_outcome_success_and_failure() {
        let hash = UInt256::default();
        let ok = ExecutionOutcome::success(hash, 1, 5_000);
        assert!(ok.ok);
        assert_eq!(ok.gas_consumed, 5_000);

        let bad = ExecutionOutcome::failure(hash, 1);
        assert!(!bad.ok);
        assert_eq!(bad.gas_consumed, 0);
    }

    #[test]
    fn validation_result_ok_and_invalid() {
        let ok = ValidationResult::ok();
        assert!(ok.valid);
        assert!(ok.reason.is_none());

        let bad = ValidationResult::invalid("bad merkle root");
        assert!(!bad.valid);
        assert_eq!(bad.reason.as_deref(), Some("bad merkle root"));
    }

    #[test]
    fn network_event_variants_are_distinct() {
        let hash = UInt256::default();
        let a = NetworkEvent::BlockReceived { block_hash: hash };
        let b = NetworkEvent::TransactionReceived { tx_hash: hash };
        assert_ne!(a, b);
    }
}
