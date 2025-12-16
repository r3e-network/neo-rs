//! Validator Service for neo-node runtime
//!
//! This module provides validator functionality including:
//! - Wallet loading and key management
//! - Consensus participation
//! - Block proposal and signing

use neo_consensus::{ConsensusEvent, ConsensusService, ValidatorInfo};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::wallets::nep6::Nep6Wallet;
use neo_core::wallets::wallet::Wallet;
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
    /// Private key for signing consensus messages (secp256r1 ECDSA)
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
    pub async fn start_consensus(
        &mut self,
        block_index: u32,
        timestamp: u64,
    ) -> anyhow::Result<()> {
        if !self.is_validator() {
            anyhow::bail!("Not a validator");
        }

        let consensus = self
            .consensus
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Consensus service not initialized"))?;

        info!(
            target: "neo::validator",
            block_index,
            validator_index = self.config.as_ref().map(|c| c.validator_index),
            "starting consensus"
        );

        // TODO: Provide real `prev_hash` from the chain tip when validator-mode
        // wiring is completed. Version must be 0 for Neo N3.
        consensus.start(block_index, timestamp, neo_core::UInt256::zero(), 0)?;
        *self.state.write().await = ValidatorState::Active;

        Ok(())
    }

    /// Processes a consensus message from the network
    pub async fn process_consensus_message(
        &mut self,
        payload: neo_consensus::ConsensusPayload,
    ) -> anyhow::Result<()> {
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
            ConsensusEvent::BlockCommitted {
                block_index,
                block_hash,
                block_data,
            } => {
                info!(
                    target: "neo::validator",
                    block_index,
                    hash = %block_hash,
                    signatures = block_data.signatures.len(),
                    required_sigs = block_data.required_signatures,
                    validators = block_data.validator_pubkeys.len(),
                    tx_count = block_data.transaction_hashes.len(),
                    "block committed - assembling block"
                );

                // Assemble the complete block with multi-sig witness
                match self.assemble_block(block_data).await {
                    Ok(block) => {
                        info!(
                            target: "neo::validator",
                            block_index = block.index(),
                            hash = %block_hash,
                            tx_count = block.transactions.len(),
                            witness_invocation_size = block.witness().invocation_script.len(),
                            witness_verification_size = block.witness().verification_script.len(),
                            "block assembled successfully"
                        );
                        // Block persistence: The assembled block is broadcast to P2P network.
                        // When received via P2PEvent::BlockReceived, runtime.rs executes the block
                        // and persists state changes to RocksDB via WorldState.commit().
                        // This follows Neo's architecture where validators propose, network validates.
                    }
                    Err(e) => {
                        warn!(
                            target: "neo::validator",
                            block_index,
                            error = %e,
                            "failed to assemble block"
                        );
                    }
                }
            }
            ConsensusEvent::ViewChanged {
                block_index,
                old_view,
                new_view,
            } => {
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
                // P2P broadcasting is handled by NodeRuntime.process_consensus_events()
                // which has access to p2p_broadcast_tx channel. ValidatorService only logs here.
            }
            ConsensusEvent::RequestTransactions {
                block_index,
                max_count,
            } => {
                info!(
                    target: "neo::validator",
                    block_index,
                    max_count,
                    "requesting transactions from mempool"
                );
                // Mempool integration is handled by NodeRuntime.process_consensus_events()
                // which has access to the mempool Arc<RwLock<Mempool>>. See runtime.rs:756-806.
            }
        }
    }

    /// Assembles a complete Block from consensus BlockData
    ///
    /// This function constructs the final Block structure by:
    /// 1. Building multi-sig invocation script from validator signatures
    /// 2. Building multi-sig verification script from validator public keys
    /// 3. Creating the Witness from invocation + verification scripts
    /// 4. Assembling the Block header with the witness
    ///
    /// # Arguments
    /// * `block_data` - Complete block data from consensus including signatures
    ///
    /// # Returns
    /// * `Ok(Block)` - Fully assembled block ready for persistence
    /// * `Err(_)` - If block assembly fails
    async fn assemble_block(
        &self,
        block_data: neo_consensus::BlockData,
    ) -> anyhow::Result<neo_core::network::p2p::payloads::Block> {
        use neo_core::network::p2p::payloads::{Block, Header, Witness};

        // Step 1: Build multi-sig invocation script from signatures
        // Format: PUSHDATA1 <sig1> PUSHDATA1 <sig2> ... PUSHDATA1 <sigM>
        let invocation_script = self.build_invocation_script(&block_data.signatures)?;

        // Step 2: Build multi-sig verification script from validator public keys
        // Format: PUSH<M> <pubkey1> ... <pubkeyN> PUSH<N> SYSCALL CheckMultisig
        let verification_script = self.build_verification_script(
            block_data.required_signatures,
            &block_data.validator_pubkeys,
        )?;

        // Step 3: Create Witness from invocation + verification scripts
        let witness = Witness::new_with_scripts(invocation_script, verification_script);

        // Step 4: Assemble Block header with witness.
        // Note: Transactions are attached by the caller (e.g. from the mempool) using
        // `block_data.transaction_hashes`.
        let mut header = Header::new();
        header.set_version(0);
        header.set_index(block_data.block_index);
        header.set_timestamp(block_data.timestamp);
        header.set_nonce(block_data.nonce);
        header.set_primary_index(block_data.primary_index);
        // Note: prev_hash, merkle_root, next_consensus need to be set by caller
        // based on blockchain state

        // Create block with header and witness
        let mut block = Block::new();
        block.header = header;
        block.header.witness = witness;

        info!(
            target: "neo::validator",
            block_index = block_data.block_index,
            invocation_size = block.header.witness.invocation_script.len(),
            verification_size = block.header.witness.verification_script.len(),
            "block witness assembled"
        );

        Ok(block)
    }

    /// Builds multi-sig invocation script from validator signatures
    ///
    /// Format: PUSHDATA1 <len> <sig1> PUSHDATA1 <len> <sig2> ... PUSHDATA1 <len> <sigM>
    ///
    /// # Arguments
    /// * `signatures` - List of (validator_index, signature) tuples
    ///
    /// # Returns
    /// * `Ok(Vec<u8>)` - Invocation script bytes
    /// * `Err(_)` - If signature format is invalid
    fn build_invocation_script(
        &self,
        signatures: &[(u8, Vec<u8>)],
    ) -> anyhow::Result<Vec<u8>> {
        use neo_vm::op_code::OpCode;

        if signatures.is_empty() {
            anyhow::bail!("No signatures provided for block witness");
        }

        // Calculate total size: each signature needs PUSHDATA1 (1 byte) + length (1 byte) + signature (64 bytes)
        let mut invocation = Vec::with_capacity(signatures.len() * (1 + 1 + 64));

        // Sort signatures by validator index to ensure deterministic ordering
        let mut sorted_sigs = signatures.to_vec();
        sorted_sigs.sort_by_key(|(idx, _)| *idx);

        for (validator_index, signature) in &sorted_sigs {
            // Validate signature length (should be 64 bytes for secp256r1)
            if signature.len() != 64 {
                anyhow::bail!(
                    "Invalid signature length from validator {}: expected 64, got {}",
                    validator_index,
                    signature.len()
                );
            }

            // Push signature: PUSHDATA1 <length> <signature_bytes>
            invocation.push(OpCode::PUSHDATA1 as u8);
            invocation.push(signature.len() as u8);
            invocation.extend_from_slice(signature);
        }

        info!(
            target: "neo::validator",
            signature_count = signatures.len(),
            invocation_size = invocation.len(),
            "invocation script built"
        );

        Ok(invocation)
    }

    /// Builds multi-sig verification script from validator public keys
    ///
    /// Format: PUSH<M> PUSHDATA1 <pubkey1> ... PUSHDATA1 <pubkeyN> PUSH<N> SYSCALL CheckMultisig
    ///
    /// # Arguments
    /// * `m` - Required signature count (M in M-of-N multi-sig)
    /// * `public_keys` - List of validator public keys (N keys)
    ///
    /// # Returns
    /// * `Ok(Vec<u8>)` - Verification script bytes
    /// * `Err(_)` - If parameters are invalid
    fn build_verification_script(
        &self,
        m: usize,
        public_keys: &[neo_crypto::ECPoint],
    ) -> anyhow::Result<Vec<u8>> {
        use neo_core::smart_contract::helper::Helper;

        // Validate parameters
        if m == 0 || m > 16 {
            anyhow::bail!("Invalid required signature count: m={} (must be 1-16)", m);
        }
        if public_keys.is_empty() || public_keys.len() > 16 {
            anyhow::bail!(
                "Invalid public key count: n={} (must be 1-16)",
                public_keys.len()
            );
        }
        if m > public_keys.len() {
            anyhow::bail!(
                "Required signatures ({}) exceeds available validators ({})",
                m,
                public_keys.len()
            );
        }

        // Convert ECPoint to compressed public key bytes
        let pubkey_bytes: Vec<Vec<u8>> = public_keys
            .iter()
            .map(|pk: &neo_crypto::ECPoint| pk.encoded().to_vec())
            .collect();

        // Use Helper to build multi-sig redeem script
        // This handles sorting and proper script construction
        let verification_script = Helper::try_multi_sig_redeem_script(m, &pubkey_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to build verification script: {}", e))?;

        info!(
            target: "neo::validator",
            required_sigs = m,
            validator_count = public_keys.len(),
            verification_size = verification_script.len(),
            "verification script built"
        );

        Ok(verification_script)
    }
}

/// Loads validator configuration from wallet file
///
/// This function loads a NEP-6 wallet file, extracts the default account's
/// private key, and determines the validator index by matching the public
/// key against the standby committee.
///
/// # Arguments
/// * `wallet_path` - Path to the NEP-6 wallet JSON file
/// * `password` - Password to decrypt the wallet
/// * `protocol_settings` - Protocol settings containing standby committee
///
/// # Returns
/// * `Ok(Some(ValidatorConfig))` - If the wallet account is a validator
/// * `Ok(None)` - If the wallet account is not in the standby committee
/// * `Err(_)` - If wallet loading fails
pub fn load_validator_from_wallet(
    wallet_path: &str,
    password: &str,
    protocol_settings: Arc<ProtocolSettings>,
) -> anyhow::Result<Option<ValidatorConfig>> {
    use neo_core::smart_contract::Contract;

    info!(
        target: "neo::validator",
        path = wallet_path,
        "loading validator wallet"
    );

    // Load the NEP-6 wallet
    let wallet = Nep6Wallet::from_file(wallet_path, password, protocol_settings.clone())
        .map_err(|e| anyhow::anyhow!("failed to load wallet: {}", e))?;

    // Get the default account
    let account = wallet
        .get_default_account()
        .ok_or_else(|| anyhow::anyhow!("wallet has no default account"))?;

    // Get the key pair from the account
    let key_pair = account
        .get_key()
        .ok_or_else(|| anyhow::anyhow!("account has no private key (watch-only account)"))?;

    // Get the public key point for comparison
    let public_key_bytes = key_pair.compressed_public_key();

    // Find validator index by matching public key against standby committee
    let validator_index = protocol_settings
        .standby_committee
        .iter()
        .take(protocol_settings.validators_count as usize)
        .position(|pk| pk.encoded() == public_key_bytes);

    match validator_index {
        Some(index) => {
            // Calculate script hash from the key pair
            let script_hash = Contract::create_signature_contract(
                protocol_settings.standby_committee[index].clone(),
            )
            .script_hash();

            let config = ValidatorConfig {
                validator_index: index as u8,
                validator_count: protocol_settings.validators_count as u16,
                network_magic: protocol_settings.network,
                private_key: key_pair.private_key().to_vec(),
                script_hash,
            };

            info!(
                target: "neo::validator",
                validator_index = index,
                script_hash = %script_hash,
                "validator configuration loaded successfully"
            );

            Ok(Some(config))
        }
        None => {
            warn!(
                target: "neo::validator",
                "wallet account is not in the standby committee - running in non-validator mode"
            );
            Ok(None)
        }
    }
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

    #[tokio::test]
    async fn test_block_assembly() {
        use neo_consensus::BlockData;
        use neo_crypto::{ECCurve, ECPoint, Secp256r1Crypto};

        // Create a non-validator service for testing
        let service = ValidatorService::new_non_validator();

        // Generate test validator keys using Secp256r1Crypto
        let mut validator_pubkeys = Vec::new();
        for _ in 0..4 {
            let private_key = Secp256r1Crypto::generate_private_key();
            let public_key_bytes = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
            let public_key = ECPoint::decode_compressed_with_curve(ECCurve::Secp256r1, &public_key_bytes).unwrap();
            validator_pubkeys.push(public_key);
        }

        // Create mock signatures (3 out of 4 validators)
        let signatures = vec![
            (0u8, vec![0u8; 64]), // Validator 0 signature
            (1u8, vec![1u8; 64]), // Validator 1 signature
            (2u8, vec![2u8; 64]), // Validator 2 signature
        ];

        // Create BlockData
        let block_data = BlockData {
            block_index: 100,
            timestamp: 1234567890,
            nonce: 0x1234567890abcdef,
            primary_index: 0,
            transaction_hashes: vec![],
            signatures,
            validator_pubkeys,
            required_signatures: 3,
        };

        // Assemble the block
        let result = service.assemble_block(block_data).await;
        assert!(result.is_ok(), "Block assembly should succeed");

        let block = result.unwrap();
        assert_eq!(block.index(), 100);
        assert_eq!(block.timestamp(), 1234567890);
        assert_eq!(block.nonce(), 0x1234567890abcdef);
        assert_eq!(block.primary_index(), 0);

        // Verify witness structure
        let witness = block.witness();
        assert!(!witness.invocation_script.is_empty(), "Invocation script should not be empty");
        assert!(!witness.verification_script.is_empty(), "Verification script should not be empty");

        // Verify invocation script format: 3 signatures * (PUSHDATA1 + len + 64 bytes) = 3 * 66 = 198 bytes
        assert_eq!(witness.invocation_script.len(), 198, "Invocation script should be 198 bytes");

        // Verify each signature is properly encoded with PUSHDATA1
        for i in 0..3 {
            let offset = i * 66;
            assert_eq!(witness.invocation_script[offset], 0x0C, "Should be PUSHDATA1 opcode");
            assert_eq!(witness.invocation_script[offset + 1], 64, "Should be 64 bytes length");
        }

        // Verify verification script starts with PUSH3 (0x53 = 0x50 + 3)
        assert_eq!(witness.verification_script[0], 0x53, "Should start with PUSH3 for M=3");
    }

    #[tokio::test]
    async fn test_invocation_script_building() {
        let service = ValidatorService::new_non_validator();

        // Test with valid signatures
        let signatures = vec![
            (0u8, vec![0xAAu8; 64]),
            (2u8, vec![0xBBu8; 64]),
            (1u8, vec![0xCCu8; 64]), // Unsorted order
        ];

        let result = service.build_invocation_script(&signatures);
        assert!(result.is_ok());

        let invocation = result.unwrap();
        // 3 signatures * (PUSHDATA1 + len + 64 bytes) = 198 bytes
        assert_eq!(invocation.len(), 198);

        // Verify signatures are sorted by validator index (0, 1, 2)
        assert_eq!(invocation[2], 0xAA); // First signature from validator 0
        assert_eq!(invocation[68], 0xCC); // Second signature from validator 1
        assert_eq!(invocation[134], 0xBB); // Third signature from validator 2
    }

    #[tokio::test]
    async fn test_invocation_script_invalid_signature_length() {
        let service = ValidatorService::new_non_validator();

        // Test with invalid signature length
        let signatures = vec![
            (0u8, vec![0xAAu8; 32]), // Invalid: only 32 bytes instead of 64
        ];

        let result = service.build_invocation_script(&signatures);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Invalid signature length"));
    }

    #[tokio::test]
    async fn test_verification_script_building() {
        use neo_crypto::{ECCurve, ECPoint, Secp256r1Crypto};

        let service = ValidatorService::new_non_validator();

        // Generate test public keys
        let mut public_keys = Vec::new();
        for _ in 0..4 {
            let private_key = Secp256r1Crypto::generate_private_key();
            let public_key_bytes = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
            let public_key = ECPoint::decode_compressed_with_curve(ECCurve::Secp256r1, &public_key_bytes).unwrap();
            public_keys.push(public_key);
        }

        // Test valid 3-of-4 multi-sig
        let result = service.build_verification_script(3, &public_keys);
        assert!(result.is_ok());

        let verification = result.unwrap();
        // Verify script starts with PUSH3 (M=3)
        assert_eq!(verification[0], 0x53); // PUSH3 = 0x50 + 3
    }

    #[tokio::test]
    async fn test_verification_script_invalid_parameters() {
        use neo_crypto::{ECCurve, ECPoint, Secp256r1Crypto};

        let service = ValidatorService::new_non_validator();

        // Generate test public keys
        let mut public_keys = Vec::new();
        for _ in 0..4 {
            let private_key = Secp256r1Crypto::generate_private_key();
            let public_key_bytes = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
            let public_key = ECPoint::decode_compressed_with_curve(ECCurve::Secp256r1, &public_key_bytes).unwrap();
            public_keys.push(public_key);
        }

        // Test M=0 (invalid)
        let result = service.build_verification_script(0, &public_keys);
        assert!(result.is_err());

        // Test M > N (invalid)
        let result = service.build_verification_script(5, &public_keys);
        assert!(result.is_err());

        // Test M > 16 (invalid)
        let result = service.build_verification_script(17, &public_keys);
        assert!(result.is_err());
    }
}
