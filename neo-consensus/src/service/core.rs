use super::ConsensusEvent;
use crate::ConsensusSigner;
use crate::context::{ConsensusContext, ValidatorInfo};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
#[cfg(test)]
use zeroize::Zeroizing;

/// The main consensus service implementing dBFT 2.0
pub struct ConsensusService {
    /// Consensus context
    pub(super) context: ConsensusContext,
    /// Network magic number
    pub(super) network: u32,
    /// Test-only private key fallback for consensus unit tests.
    /// Production builds can sign only through `ConsensusSigner`.
    #[cfg(test)]
    pub(super) private_key: Zeroizing<Vec<u8>>,
    /// Optional signer for consensus messages (wallet/HSM/external signer).
    pub(super) signer: Option<Arc<dyn ConsensusSigner>>,
    /// Event sender
    pub(super) event_tx: mpsc::Sender<ConsensusEvent>,
    /// Whether the service is running
    pub(super) running: bool,
    /// True only while replaying a RecoveryMessage payload set.
    pub(super) is_recovering: bool,
    /// Current-view commits received before the proposal block hash is known.
    ///
    /// C# stores these payloads and re-verifies them once the header exists
    /// (`OnMessage.cs:122-130`). Dropping them made a late/restarted node
    /// unable to reach M commits after recovery installed the proposal.
    /// Key: validator index; value: (view, signature, invocation script).
    pub(super) pending_commits: HashMap<u8, (u8, Vec<u8>, Vec<u8>)>,
}

impl ConsensusService {
    /// Creates a new consensus service
    #[must_use]
    pub fn new(
        network: u32,
        validators: Vec<ValidatorInfo>,
        my_index: Option<u8>,
        private_key: Vec<u8>,
        event_tx: mpsc::Sender<ConsensusEvent>,
    ) -> Self {
        #[cfg(not(test))]
        let _ = private_key;
        Self {
            context: ConsensusContext::new(0, validators, my_index, None),
            network,
            #[cfg(test)]
            private_key: Zeroizing::new(private_key),
            signer: None,
            event_tx,
            running: false,
            is_recovering: false,
            pending_commits: HashMap::new(),
        }
    }

    /// Creates a consensus service from a pre-loaded context (recovery logs).
    #[must_use]
    pub fn with_context(
        network: u32,
        context: ConsensusContext,
        private_key: Vec<u8>,
        event_tx: mpsc::Sender<ConsensusEvent>,
    ) -> Self {
        #[cfg(not(test))]
        let _ = private_key;
        Self {
            context,
            network,
            #[cfg(test)]
            private_key: Zeroizing::new(private_key),
            signer: None,
            event_tx,
            running: false,
            is_recovering: false,
            pending_commits: HashMap::new(),
        }
    }
}
