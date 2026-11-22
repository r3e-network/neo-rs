use super::*;

use neo_consensus::{
    CommitPersistencePermit, CommitPersistencePort, ConsensusContext, ProposalTransactionVerdict,
    RecoveryLoadOutcome, RejectionCause, evaluate_recovery_context,
};
use tracing::{error, warn};

impl ConsensusActor {
    pub(super) fn load_recovery_from_store(
        &self,
        validator_infos: Vec<ValidatorInfo>,
        my_index: Option<u8>,
    ) -> Option<ConsensusContext> {
        let store = self.recovery_store.as_ref()?;
        let snapshot = store.snapshot();
        let key = CONSENSUS_STATE_KEY.to_vec();
        let data = snapshot.try_get(&key)?;
        match ConsensusContext::from_bytes(&data, validator_infos, my_index) {
            Ok(context) => Some(context),
            Err(err) => {
                warn!(target: "neo", %err, "failed to decode consensus recovery log");
                None
            }
        }
    }

    fn persist_commit_recovery_state(&self, permit: &CommitPersistencePermit) -> bool {
        if permit.validate().is_err() {
            return false;
        }

        if let Some(store) = self.recovery_store.as_ref() {
            let mut snapshot = store.snapshot();
            if let Some(snap) = Arc::get_mut(&mut snapshot)
                && snap
                    .put_sync(CONSENSUS_STATE_KEY.to_vec(), permit.recovery_state.clone())
                    .is_ok()
                && snap.try_commit_durable().is_ok()
                && store
                    .snapshot()
                    .try_get(&CONSENSUS_STATE_KEY.to_vec())
                    .as_deref()
                    == Some(permit.recovery_state.as_slice())
            {
                return true;
            }
            error!(target: "neo", "durable consensus recovery store write failed; withholding signature egress");
            return false;
        }

        let fallback_path = if self.recovery_path.is_dir() {
            self.recovery_path.join("consensus.context")
        } else {
            self.recovery_path.clone()
        };
        match ConsensusContext::save_bytes_durable(&fallback_path, &permit.recovery_state) {
            Ok(()) => true,
            Err(err) => {
                error!(target: "neo", %err, "commit recovery state could not be durably persisted; withholding signature egress");
                false
            }
        }
    }
}

/// Node adapter for [`CommitPersistencePort`]: durable recovery store first,
/// durable-file fallback otherwise.
impl CommitPersistencePort for ConsensusActor {
    fn persist_commit_permit(&self, permit: &CommitPersistencePermit) -> bool {
        self.persist_commit_recovery_state(permit)
    }
}

/// Accepts a loaded recovery context only when it belongs to `block_index`.
pub(super) fn accept_recovery_context(
    loaded: Option<ConsensusContext>,
    block_index: u32,
) -> Option<ConsensusContext> {
    let context = loaded?;
    match evaluate_recovery_context(&context, block_index) {
        RecoveryLoadOutcome::Resume => Some(context),
        RecoveryLoadOutcome::HeightMismatch {
            recovered,
            expected,
        } => {
            warn!(
                target: "neo",
                recovered,
                expected,
                "recovery log does not match current height; ignoring"
            );
            None
        }
    }
}

/// Applies a proposal verdict: logs, records the rejection and requests the
/// view change mandated by the policy. Returns `true` when the transaction was
/// rejected (the caller must stop processing the proposal).
pub(super) fn apply_proposal_verdict(
    service: &mut ConsensusService,
    hash: &UInt256,
    verdict: ProposalTransactionVerdict,
    now: u64,
) -> bool {
    let ProposalTransactionVerdict::Reject { reason, cause } = verdict else {
        return false;
    };
    log_proposal_rejection(hash, cause);
    service.record_rejected_transaction(*hash);
    let _ = service.request_change_view(reason, now);
    true
}

fn log_proposal_rejection(hash: &UInt256, cause: RejectionCause) {
    match cause {
        RejectionCause::AlreadyOnChain => warn!(
            target: "neo",
            tx_hash = %hash,
            "prepare request includes on-chain transaction"
        ),
        RejectionCause::LedgerConflict => warn!(
            target: "neo",
            tx_hash = %hash,
            "prepare request contains conflicting transaction"
        ),
        RejectionCause::SystemFeeExceeded => warn!(
            target: "neo",
            tx_hash = %hash,
            "transaction system fee exceeds max_block_system_fee"
        ),
        RejectionCause::ProposalConflict => warn!(
            target: "neo",
            tx_hash = %hash,
            "transaction conflicts with existing set"
        ),
        // Verification failures are logged with their validation result where
        // the deterministic check ran.
        RejectionCause::VerificationPolicy | RejectionCause::VerificationInvalid => {}
    }
}
pub(super) fn resolve_recovery_path(value: &str) -> PathBuf {
    // Return the path as-is - PathBuf handles both absolute and relative paths
    PathBuf::from(value)
}

pub(super) fn open_recovery_store(
    system: &neo_system::NeoSystem,
    settings: &DbftSettings,
) -> Option<Arc<dyn Store>> {
    if settings.ignore_recovery_logs {
        return None;
    }
    let path = settings.recovery_logs.trim();
    if path.is_empty() {
        return None;
    }
    match system.store_provider().get_store(path) {
        Ok(store) if store.supports_durable_writes() => Some(store),
        Ok(_) => {
            warn!(target: "neo", "consensus recovery store lacks durable writes; using durable file mode");
            None
        }
        Err(err) => {
            warn!(target: "neo", %err, "failed to open consensus recovery store; using durable file mode");
            None
        }
    }
}
