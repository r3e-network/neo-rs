use super::ConsensusEvent;
use crate::context::{ConsensusContext, ValidatorInfo};
use crate::{ConsensusSigner, NoConsensusSigner};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

/// Neo N3 v3.10.1 `ProtocolSettings.Default.MaxTransactionsPerBlock`.
pub(super) const DEFAULT_MAX_TRANSACTIONS_PER_BLOCK: u32 = 512;

/// The main consensus service implementing dBFT 2.0
pub struct ConsensusService<S = NoConsensusSigner>
where
    S: ConsensusSigner,
{
    /// Consensus context
    pub(super) context: ConsensusContext,
    /// Network magic number
    pub(super) network: u32,
    /// Private key for signing consensus messages (secp256r1 ECDSA).
    /// Wrapped in `Zeroizing` so key material is wiped from memory on drop.
    pub(super) private_key: Zeroizing<Vec<u8>>,
    /// Optional signer for consensus messages (wallet/HSM/external signer).
    pub(super) signer: Option<Arc<S>>,
    /// Event sender
    pub(super) event_tx: mpsc::Sender<ConsensusEvent>,
    /// Protocol `MaxTransactionsPerBlock` limit used for proposal assembly and validation.
    pub(super) max_transactions_per_block: u32,
    /// Whether the service is running
    pub(super) running: bool,
    /// Optional recovery-log file path. When set, the consensus context is
    /// persisted to this file immediately before this node signs and broadcasts
    /// its own Commit (C# `ConsensusService.CheckPreparations` -> `context.Save()`
    /// before `localNode.Tell(payload)`), and reloaded on startup so a crash /
    /// restart cannot double-sign a different block at the same (height, view).
    /// `None` disables persistence (C# `DbftSettings.IgnoreRecoveryLogs = true`).
    pub(super) state_path: Option<PathBuf>,
}

impl ConsensusService<NoConsensusSigner> {
    /// Creates a new consensus service
    #[must_use]
    pub fn new(
        network: u32,
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
        private_key: Vec<u8>,
        event_tx: mpsc::Sender<ConsensusEvent>,
    ) -> Self {
        Self::new_with_signer(network, validators, my_index, private_key, event_tx, None)
    }

    /// Creates a consensus service from a pre-loaded context (recovery logs).
    #[must_use]
    pub fn with_context(
        network: u32,
        context: ConsensusContext,
        private_key: Vec<u8>,
        event_tx: mpsc::Sender<ConsensusEvent>,
    ) -> Self {
        Self::with_context_and_signer(network, context, private_key, event_tx, None)
    }
}

impl<S> ConsensusService<S>
where
    S: ConsensusSigner,
{
    /// Creates a new consensus service with a concrete external signer type.
    #[must_use]
    pub fn new_with_signer(
        network: u32,
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
        private_key: Vec<u8>,
        event_tx: mpsc::Sender<ConsensusEvent>,
        signer: Option<Arc<S>>,
    ) -> Self {
        Self {
            context: ConsensusContext::new(0, validators, my_index, None),
            network,
            private_key: Zeroizing::new(private_key),
            signer,
            event_tx,
            max_transactions_per_block: DEFAULT_MAX_TRANSACTIONS_PER_BLOCK,
            running: false,
            state_path: None,
        }
    }

    /// Creates a consensus service from a pre-loaded context and a concrete
    /// external signer type.
    #[must_use]
    pub fn with_context_and_signer(
        network: u32,
        context: ConsensusContext,
        private_key: Vec<u8>,
        event_tx: mpsc::Sender<ConsensusEvent>,
        signer: Option<Arc<S>>,
    ) -> Self {
        Self {
            context,
            network,
            private_key: Zeroizing::new(private_key),
            signer,
            event_tx,
            max_transactions_per_block: DEFAULT_MAX_TRANSACTIONS_PER_BLOCK,
            running: false,
            state_path: None,
        }
    }
}
