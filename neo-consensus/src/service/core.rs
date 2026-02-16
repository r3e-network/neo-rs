use super::ConsensusEvent;
use crate::ConsensusSigner;
use crate::context::{ConsensusContext, ValidatorInfo};
use std::sync::Arc;
use tokio::sync::mpsc;
use zeroize::Zeroizing;

/// The main consensus service implementing dBFT 2.0
pub struct ConsensusService {
    /// Consensus context
    pub(super) context: ConsensusContext,
    /// Network magic number
    pub(super) network: u32,
    /// Private key for signing consensus messages (secp256r1 ECDSA).
    /// Wrapped in `Zeroizing` so key material is wiped from memory on drop.
    #[allow(dead_code)]
    pub(super) private_key: Zeroizing<Vec<u8>>,
    /// Optional signer for consensus messages (wallet/HSM/external signer).
    pub(super) signer: Option<Arc<dyn ConsensusSigner>>,
    /// Event sender
    pub(super) event_tx: mpsc::Sender<ConsensusEvent>,
    /// Whether the service is running
    pub(super) running: bool,
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
        Self {
            context: ConsensusContext::new(0, validators, my_index),
            network,
            private_key: Zeroizing::new(private_key),
            signer: None,
            event_tx,
            running: false,
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
        Self {
            context,
            network,
            private_key: Zeroizing::new(private_key),
            signer: None,
            event_tx,
            running: false,
        }
    }
}
