use super::ConsensusEvent;
use crate::context::{ConsensusContext, ValidatorInfo};
use tokio::sync::mpsc;

/// The main consensus service implementing dBFT 2.0
pub struct ConsensusService {
    /// Consensus context
    pub(super) context: ConsensusContext,
    /// Network magic number
    pub(super) network: u32,
    /// Private key for signing consensus messages (secp256r1 ECDSA)
    #[allow(dead_code)]
    pub(super) private_key: Vec<u8>,
    /// Event sender
    pub(super) event_tx: mpsc::Sender<ConsensusEvent>,
    /// Whether the service is running
    pub(super) running: bool,
}

impl ConsensusService {
    /// Creates a new consensus service
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
            private_key,
            event_tx,
            running: false,
        }
    }
}
