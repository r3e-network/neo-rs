//! Commit-egress authorization policy.
//!
//! A local `Commit` (or the recovery record that carries it) may only leave
//! the node after its recovery record has been durably persisted. These rules
//! are pure consensus decisions; durability itself is reached through
//! [`CommitPersistencePort`], which the node adapter implements on top of its
//! recovery store (or durable-file fallback).

use crate::ConsensusMessageType;
use crate::context::CommitPersistencePermit;
use crate::messages::{ConsensusPayload, RecoveryMessage};
use crate::service::BlockData;

/// Port for durable commit-recovery persistence.
///
/// Implementations must persist `permit.recovery_state` and verify it can be
/// read back. Returning `false` withholds every signature egress bound to that
/// permit, so failures always fail closed.
pub trait CommitPersistencePort {
    /// Durably stores the recovery record carried by `permit`.
    fn persist_commit_permit(&self, permit: &CommitPersistencePermit) -> bool;
}

/// Authorization decisions for Commit-bound signature egress.
pub struct CommitEgressGuard;

impl CommitEgressGuard {
    /// Authorizes a single Commit (broadcast or block-witness signature).
    ///
    /// The permit must cover exactly this `(block, view, validator, signature)`
    /// tuple and the recovery record must be durably persisted before the
    /// signature is released.
    pub fn authorize_commit(
        permit: Option<&CommitPersistencePermit>,
        block_index: u32,
        view_number: u8,
        validator_index: u8,
        signature: &[u8],
        port: &dyn CommitPersistencePort,
    ) -> bool {
        let Some(permit) = permit else {
            return false;
        };
        permit.covers_commit(block_index, view_number, validator_index, signature)
            && port.persist_commit_permit(permit)
    }

    /// Authorizes broadcasting a consensus payload.
    ///
    /// Only the local `Commit` and a `RecoveryMessage` echoing the local
    /// Commit are gated; every other message type passes through untouched.
    pub fn authorize_payload(
        payload: &ConsensusPayload,
        my_index: Option<u8>,
        port: &dyn CommitPersistencePort,
    ) -> bool {
        match payload.message_type {
            ConsensusMessageType::Commit if my_index == Some(payload.validator_index) => {
                Self::authorize_commit(
                    payload.commit_persistence.as_ref(),
                    payload.block_index,
                    payload.view_number,
                    payload.validator_index,
                    &payload.data,
                    port,
                )
            }
            ConsensusMessageType::RecoveryMessage => {
                let Ok(recovery) = RecoveryMessage::deserialize(
                    &payload.data,
                    payload.block_index,
                    payload.view_number,
                    payload.validator_index,
                ) else {
                    return false;
                };
                let Some(local_commit) = my_index.and_then(|local_index| {
                    recovery
                        .commit_messages
                        .iter()
                        .find(|commit| commit.validator_index == local_index)
                }) else {
                    return true;
                };
                Self::authorize_commit(
                    payload.commit_persistence.as_ref(),
                    payload.block_index,
                    local_commit.view_number,
                    local_commit.validator_index,
                    &local_commit.signature,
                    port,
                )
            }
            _ => true,
        }
    }

    /// Authorizes handing a fully committed block to the ledger.
    ///
    /// The block carries the local Commit signature, so the same permit check
    /// applies; blocks without a local signature are not gated.
    pub fn authorize_block(
        block_data: &BlockData,
        my_index: Option<u8>,
        port: &dyn CommitPersistencePort,
    ) -> bool {
        let Some(local_index) = my_index else {
            return true;
        };
        let Some((_, signature)) = block_data
            .signatures
            .iter()
            .find(|(validator_index, _)| *validator_index == local_index)
        else {
            return true;
        };
        let view_number = block_data
            .commit_persistence
            .as_ref()
            .map_or(0, |permit| permit.view_number);
        Self::authorize_commit(
            block_data.commit_persistence.as_ref(),
            block_data.block_index,
            view_number,
            local_index,
            signature,
            port,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ConsensusContext, ValidatorInfo};
    use crate::messages::{CommitPayloadCompact, RecoveryMessage};
    use neo_primitives::{UInt160, UInt256};
    use std::cell::Cell;

    struct RecordingPort {
        calls: Cell<usize>,
        result: bool,
    }

    impl RecordingPort {
        fn new(result: bool) -> Self {
            Self {
                calls: Cell::new(0),
                result,
            }
        }
    }

    impl CommitPersistencePort for RecordingPort {
        fn persist_commit_permit(&self, _permit: &CommitPersistencePermit) -> bool {
            self.calls.set(self.calls.get() + 1);
            self.result
        }
    }

    fn valid_commit_permit() -> CommitPersistencePermit {
        let validators = vec![ValidatorInfo {
            index: 0,
            public_key: neo_crypto::ECPoint::infinity(neo_crypto::ECCurve::Secp256r1),
            script_hash: UInt160::zero(),
        }];
        let mut context = ConsensusContext::new(42, validators, Some(0), None);
        context.proposed_block_hash = Some(UInt256::from([0x42; 32]));
        context
            .add_commit(0, 0, vec![0xA5; 64])
            .expect("add local commit");
        context
            .commit_persistence_permit()
            .expect("build commit permit")
            .expect("local commit permit")
    }

    #[test]
    fn commit_without_permit_has_no_consensus_egress() {
        let network = 860_833_102;
        let recovered_signature = vec![0x5A; 64];
        let commit_payload = ConsensusPayload::new(
            network,
            42,
            0,
            3,
            ConsensusMessageType::Commit,
            recovered_signature.clone(),
        );
        let mut recovery = RecoveryMessage::new(42, 3, 0);
        recovery.commit_messages.push(CommitPayloadCompact {
            view_number: 3,
            validator_index: 0,
            signature: recovered_signature.clone(),
            invocation_script: Vec::new(),
        });
        let recovery_payload = ConsensusPayload::new(
            network,
            42,
            0,
            3,
            ConsensusMessageType::RecoveryMessage,
            recovery.serialize(),
        );
        let block_data = BlockData {
            block_index: 42,
            timestamp: 0,
            nonce: 0,
            primary_index: 0,
            transaction_hashes: Vec::new(),
            signatures: vec![(0, recovered_signature)],
            validator_pubkeys: vec![neo_crypto::ECPoint::infinity(
                neo_crypto::ECCurve::Secp256r1,
            )],
            required_signatures: 1,
            commit_persistence: None,
        };
        let port = RecordingPort::new(true);

        let commit_broadcast =
            CommitEgressGuard::authorize_payload(&commit_payload, Some(0), &port);
        let recovery_broadcast =
            CommitEgressGuard::authorize_payload(&recovery_payload, Some(0), &port);
        let block_persistence = CommitEgressGuard::authorize_block(&block_data, Some(0), &port);

        assert!(!commit_broadcast, "Commit broadcast requires its permit");
        assert!(
            !recovery_broadcast,
            "RecoveryMessage carrying the recovered Commit requires its permit"
        );
        assert!(
            !block_persistence,
            "final block persistence requires the local Commit permit"
        );
        assert_eq!(
            port.calls.get(),
            0,
            "missing permits must fail before durable persistence"
        );
    }

    #[test]
    fn commit_egress_fails_closed_when_persistence_fails() {
        let permit = valid_commit_permit();
        let port = RecordingPort::new(false);

        let authorized = CommitEgressGuard::authorize_commit(
            Some(&permit),
            permit.block_index,
            permit.view_number,
            permit.validator_index,
            &permit.signature,
            &port,
        );

        assert_eq!(
            port.calls.get(),
            1,
            "valid permit must reach durable persistence"
        );
        assert!(
            !authorized,
            "signature egress must be withheld after persistence failure"
        );
    }

    #[test]
    fn commit_egress_passes_when_permit_is_persisted() {
        let permit = valid_commit_permit();
        let port = RecordingPort::new(true);

        let authorized = CommitEgressGuard::authorize_commit(
            Some(&permit),
            permit.block_index,
            permit.view_number,
            permit.validator_index,
            &permit.signature,
            &port,
        );

        assert!(authorized);
    }

    #[test]
    fn abandoned_permit_does_not_authorize_a_different_signature() {
        let permit = valid_commit_permit();
        let port = RecordingPort::new(true);

        let authorized = CommitEgressGuard::authorize_commit(
            Some(&permit),
            permit.block_index,
            permit.view_number,
            permit.validator_index,
            &[0x11; 64],
            &port,
        );

        assert!(!authorized, "a permit only covers its exact signature");
        assert_eq!(port.calls.get(), 0, "mismatched permits never persist");
    }
}
