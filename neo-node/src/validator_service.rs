//! Validator Service for neo-node runtime
//!
//! This module provides validator functionality including:
//! - Wallet loading and key management
//! - Consensus participation
//! - Block proposal and signing

use neo_consensus::{ConsensusEvent, ConsensusService, ValidatorInfo};
use neo_core::UInt160;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{info, warn};

/// Validator service state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidatorState {
    /// Not a validator
    Inactive,
    /// Validator but not participating
    Standby,
    /// Actively participating in consensus
    Active,
}

/// Validator configuration
#[derive(Debug, Clone)]
pub struct ValidatorConfig {
    /// Validator index in the committee
    pub validator_index: u8,
    /// Total number of validators
    pub validator_count: u16,
    /// Network magic
    pub network_magic: u32,
    /// Private key for signing (in production, use secure key management)
    pub private_key: Vec<u8>,
    /// Script hash of the validator account
    pub script_hash: UInt160,
}

/// Validator Service
pub struct ValidatorService {
    /// Current state
    state: Arc<RwLock<ValidatorState>>,
    /// Configuration (if validator)
    config: Option<ValidatorConfig>,
    /// Consensus service
    consensus: Option<ConsensusService>,
    /// Consensus event receiver
    consensus_rx: Option<mpsc::Receiver<ConsensusEvent>>,
    /// Shutdown signal
    shutdown_tx: broadcast::Sender<()>,
}

impl ValidatorService {
    /// Creates a new validator service (non-validator mode)
    pub fn new_non_validator() -> Self {
        let (shutdown_tx, _) = broadcast::channel(8);

        Self {
            state: Arc::new(RwLock::new(ValidatorState::Inactive)),
            config: None,
            consensus: None,
            consensus_rx: None,
            shutdown_tx,
        }
    }

    /// Creates a new validator service with configuration
    pub fn new_validator(
        config: ValidatorConfig,
        validators: Vec<ValidatorInfo>,
    ) -> (Self, mpsc::Sender<ConsensusEvent>) {
        let (shutdown_tx, _) = broadcast::channel(8);
        let (consensus_tx, consensus_rx) = mpsc::channel(128);

        let consensus = ConsensusService::new(
            config.network_magic,
            validators,
            Some(config.validator_index),
            config.private_key.clone(),
            consensus_tx.clone(),
        );

        let service = Self {
            state: Arc::new(RwLock::new(ValidatorState::Standby)),
            config: Some(config),
            consensus: Some(consensus),
            consensus_rx: Some(consensus_rx),
            shutdown_tx,
        };

        (service, consensus_tx)
    }

    /// Returns the current validator state
    pub async fn state(&self) -> ValidatorState {
        *self.state.read().await
    }

    /// Returns true if this node is a validator
    pub fn is_validator(&self) -> bool {
        self.config.is_some()
    }

    /// Returns the validator index if this is a validator
    pub fn validator_index(&self) -> Option<u8> {
        self.config.as_ref().map(|c| c.validator_index)
    }

    /// Starts consensus participation
    pub async fn start_consensus(&mut self, block_index: u32, timestamp: u64) -> anyhow::Result<()> {
        if !self.is_validator() {
            anyhow::bail!("Not a validator");
        }

        let consensus = self.consensus.as_mut().ok_or_else(|| {
            anyhow::anyhow!("Consensus service not initialized")
        })?;

        info!(
            target: "neo::validator",
            block_index,
            validator_index = self.config.as_ref().map(|c| c.validator_index),
            "starting consensus"
        );

        consensus.start(block_index, timestamp)?;
        *self.state.write().await = ValidatorState::Active;

        Ok(())
    }

    /// Processes a consensus message from the network
    pub async fn process_consensus_message(&mut self, payload: neo_consensus::ConsensusPayload) -> anyhow::Result<()> {
        if let Some(consensus) = self.consensus.as_mut() {
            consensus.process_message(payload)?;
        }
        Ok(())
    }

    /// Handles timer tick for consensus timeouts
    pub async fn on_timer_tick(&mut self, timestamp: u64) -> anyhow::Result<()> {
        if let Some(consensus) = self.consensus.as_mut() {
            consensus.on_timer_tick(timestamp)?;
        }
        Ok(())
    }

    /// Stops the validator service
    pub async fn stop(&self) {
        let _ = self.shutdown_tx.send(());
        info!(target: "neo::validator", "validator service stopped");
    }

    /// Runs the validator event loop
    pub async fn run(&mut self) -> anyhow::Result<()> {
        if !self.is_validator() {
            return Ok(());
        }

        let mut shutdown_rx = self.shutdown_tx.subscribe();
        let consensus_rx = self.consensus_rx.take();

        if let Some(mut rx) = consensus_rx {
            info!(target: "neo::validator", "validator event loop started");

            loop {
                tokio::select! {
                    Some(event) = rx.recv() => {
                        self.handle_consensus_event(event).await;
                    }
                    _ = shutdown_rx.recv() => {
                        info!(target: "neo::validator", "validator event loop stopping");
                        break;
                    }
                }
            }
        }

        Ok(())
    }

    /// Handles a consensus event
    async fn handle_consensus_event(&self, event: ConsensusEvent) {
        match event {
            ConsensusEvent::BlockCommitted { block_index, block_hash, signatures } => {
                info!(
                    target: "neo::validator",
                    block_index,
                    hash = %block_hash,
                    signatures = signatures.len(),
                    "block committed"
                );
            }
            ConsensusEvent::ViewChanged { block_index, old_view, new_view } => {
                info!(
                    target: "neo::validator",
                    block_index,
                    old_view,
                    new_view,
                    "view changed"
                );
            }
            ConsensusEvent::BroadcastMessage(payload) => {
                info!(
                    target: "neo::validator",
                    block_index = payload.block_index,
                    msg_type = ?payload.message_type,
                    "broadcasting consensus message"
                );
                // TODO: Send to P2P layer
            }
            ConsensusEvent::RequestTransactions { block_index, max_count } => {
                info!(
                    target: "neo::validator",
                    block_index,
                    max_count,
                    "requesting transactions from mempool"
                );
                // TODO: Get transactions from mempool
            }
        }
    }
}

/// Loads validator configuration from wallet file
pub fn load_validator_from_wallet(
    _wallet_path: &str,
    _password: &str,
    _network_magic: u32,
) -> anyhow::Result<Option<ValidatorConfig>> {
    // TODO: Implement actual wallet loading
    // For now, return None (non-validator mode)
    warn!(target: "neo::validator", "wallet loading not yet implemented");
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_non_validator_creation() {
        let service = ValidatorService::new_non_validator();

        assert_eq!(service.state().await, ValidatorState::Inactive);
        assert!(!service.is_validator());
        assert!(service.validator_index().is_none());
    }

    #[tokio::test]
    async fn test_validator_creation() {
        use neo_core::{ECCurve, ECPoint};

        let config = ValidatorConfig {
            validator_index: 0,
            validator_count: 7,
            network_magic: 0x4F454E,
            private_key: vec![0u8; 32],
            script_hash: UInt160::zero(),
        };

        let validators: Vec<ValidatorInfo> = (0..7)
            .map(|i| ValidatorInfo {
                index: i,
                public_key: ECPoint::infinity(ECCurve::Secp256r1),
                script_hash: UInt160::zero(),
            })
            .collect();

        let (service, _tx) = ValidatorService::new_validator(config, validators);

        assert_eq!(service.state().await, ValidatorState::Standby);
        assert!(service.is_validator());
        assert_eq!(service.validator_index(), Some(0));
    }
}
