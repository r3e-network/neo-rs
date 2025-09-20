//! Consensus integration module
//!
//! This module provides integration between the consensus service and the actual blockchain

use crate::native_contracts::NativeContractsManager;
use async_trait::async_trait;
use neo_consensus::{Error as ConsensusError, Result as ConsensusResult};
use neo_core::{Block, Transaction, UInt160, UInt256, Witness};
use neo_ledger::Blockchain;
use neo_network::{
    messages::{ExtensiblePayload, NetworkMessage, ProtocolMessage},
    P2pNode,
};
use once_cell::sync::Lazy;
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Converts a neo_ledger::Block to neo_core::Block
fn convert_ledger_block_to_core(ledger_block: &neo_ledger::Block) -> neo_core::Block {
    let header = neo_core::BlockHeader {
        version: ledger_block.header.version,
        previous_hash: ledger_block.header.previous_hash,
        merkle_root: ledger_block.header.merkle_root,
        timestamp: ledger_block.header.timestamp,
        nonce: ledger_block.header.nonce,
        index: ledger_block.header.index,
        primary_index: ledger_block.header.primary_index,
        next_consensus: ledger_block.header.next_consensus,
        witnesses: ledger_block.header.witnesses.clone(),
    };

    neo_core::Block {
        header,
        transactions: ledger_block.transactions.clone(),
    }
}

/// Converts a neo_core::Block to neo_ledger::Block  
fn convert_core_block_to_ledger(core_block: &neo_core::Block) -> neo_ledger::Block {
    let mut header = neo_ledger::BlockHeader::new(
        core_block.header.version,
        core_block.header.previous_hash,
        core_block.header.merkle_root,
        core_block.header.timestamp,
        core_block.header.nonce,
        core_block.header.index,
        core_block.header.primary_index,
        core_block.header.next_consensus,
    );

    // Set the witnesses separately since they're not part of the constructor
    header.witnesses = core_block.header.witnesses.clone();

    neo_ledger::Block::new(header, core_block.transactions.clone())
}

static GAS_CONTRACT_HASH: Lazy<UInt160> = Lazy::new(|| {
    UInt160::from_str("d2a4cff31913016155e38e474a2c06d08be276cf")
        .expect("valid GAS contract hash")
});

/// Consensus ledger adapter that bridges consensus to the actual blockchain
pub struct ConsensusLedgerAdapter {
    blockchain: Arc<Blockchain>,
    native_contracts: Option<Arc<NativeContractsManager>>,
}

impl ConsensusLedgerAdapter {
    pub fn new(blockchain: Arc<Blockchain>) -> Self {
        Self {
            blockchain,
            native_contracts: None,
        }
    }

    /// Create a new adapter with native contracts support
    pub fn new_with_native_contracts(
        blockchain: Arc<Blockchain>,
        native_contracts: Arc<NativeContractsManager>,
    ) -> Self {
        Self {
            blockchain,
            native_contracts: Some(native_contracts),
        }
    }
}

#[async_trait]
impl neo_consensus::LedgerService for ConsensusLedgerAdapter {
    async fn get_block(&self, height: u32) -> ConsensusResult<Option<Block>> {
        match self.blockchain.get_block(height).await {
            Ok(Some(ledger_block)) => {
                let core_block = convert_ledger_block_to_core(&ledger_block);
                Ok(Some(core_block))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(ConsensusError::Ledger(format!(
                "Failed to get block: {}",
                e
            ))),
        }
    }

    async fn get_block_by_hash(&self, hash: &UInt256) -> ConsensusResult<Option<Block>> {
        match self.blockchain.get_block_by_hash(hash).await {
            Ok(Some(ledger_block)) => {
                let core_block = convert_ledger_block_to_core(&ledger_block);
                Ok(Some(core_block))
            }
            Ok(None) => Ok(None),
            Err(e) => Err(ConsensusError::Ledger(format!(
                "Failed to get block by hash: {}",
                e
            ))),
        }
    }

    async fn get_current_height(&self) -> ConsensusResult<u32> {
        Ok(self.blockchain.get_height().await)
    }

    async fn add_block(&self, block: Block) -> ConsensusResult<()> {
        let ledger_block = convert_core_block_to_ledger(&block);

        match self.blockchain.persist_block(&ledger_block).await {
            Ok(()) => {
                tracing::info!(
                    "Successfully added block {} to blockchain",
                    block.header.index
                );
                Ok(())
            }
            Err(e) => {
                tracing::error!(
                    "Failed to add block {} to blockchain: {}",
                    block.header.index,
                    e
                );
                Err(ConsensusError::Ledger(format!(
                    "Failed to add block: {}",
                    e
                )))
            }
        }
    }

    async fn get_transaction(&self, hash: &UInt256) -> ConsensusResult<Option<Transaction>> {
        match self.blockchain.get_transaction(hash).await {
            Ok(transaction) => Ok(transaction),
            Err(e) => Err(ConsensusError::Ledger(format!(
                "Failed to get transaction: {}",
                e
            ))),
        }
    }

    async fn contains_transaction(&self, hash: &UInt256) -> ConsensusResult<bool> {
        self.blockchain
            .contains_transaction(hash)
            .await
            .map_err(|e| ConsensusError::Ledger(format!("Failed to check transaction: {}", e)))
    }

    async fn get_next_block_validators(&self) -> ConsensusResult<Vec<neo_cryptography::ECPoint>> {
        if let Some(native_contracts) = &self.native_contracts {
            match native_contracts.neo.get_next_block_validators().await {
                Ok(validators) => {
                    tracing::debug!(
                        "Retrieved {} validators from NEO contract",
                        validators.len()
                    );
                    Ok(validators)
                }
                Err(e) => {
                    tracing::warn!("Failed to get validators from NEO contract: {}", e);
                    Ok(vec![])
                }
            }
        } else {
            tracing::debug!(
                "get_next_block_validators called - returning empty set (native contracts not initialized)"
            );
            Ok(vec![])
        }
    }

    async fn get_validators(&self, height: u32) -> ConsensusResult<Vec<neo_cryptography::ECPoint>> {
        if let Some(native_contracts) = &self.native_contracts {
            match native_contracts.neo.get_validators_at_height(height).await {
                Ok(validators) => {
                    tracing::debug!(
                        "Retrieved {} validators for height {} from NEO contract",
                        validators.len(),
                        height
                    );
                    Ok(validators)
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to get validators for height {} from NEO contract: {}",
                        height,
                        e
                    );
                    Ok(vec![])
                }
            }
        } else {
            tracing::debug!(
                "get_validators called for height {} - returning empty set (native contracts not initialized)",
                height
            );
            Ok(vec![])
        }
    }

    async fn validate_transaction(&self, transaction: &Transaction) -> ConsensusResult<bool> {
        match self.blockchain.validate_transaction(transaction).await {
            Ok(is_valid) => {
                if is_valid {
                    tracing::debug!(
                        "Transaction {} validation passed",
                        transaction.hash().unwrap_or_default()
                    );
                } else {
                    tracing::debug!(
                        "Transaction {} validation failed",
                        transaction.hash().unwrap_or_default()
                    );
                }
                Ok(is_valid)
            }
            Err(e) => {
                tracing::error!("Transaction validation error: {}", e);
                Err(ConsensusError::Ledger(format!(
                    "Transaction validation failed: {}",
                    e
                )))
            }
        }
    }

    async fn get_account_balance(&self, account: &UInt160) -> ConsensusResult<u64> {
        if let Some(native_contracts) = &self.native_contracts {
            return native_contracts
                .gas
                .get_balance(account)
                .await
                .map_err(|e| ConsensusError::Ledger(format!(
                    "Failed to get GAS balance via native contract: {}",
                    e
                )));
        }

        let balance = self
            .blockchain
            .get_nep17_balance(&GAS_CONTRACT_HASH, account)
            .await
            .map_err(|e| {
                ConsensusError::Ledger(format!("Failed to read GAS balance from storage: {}", e))
            })?;

        Ok(balance.amount.min(u128::from(u64::MAX)) as u64)
    }
}

#[async_trait]
impl neo_consensus::NetworkService for ConsensusNetworkAdapter {
    async fn broadcast_consensus_message(&self, message: Vec<u8>) -> ConsensusResult<()> {
        let payload = ExtensiblePayload::new(
            "dBFT".to_string(),
            0,
            u32::MAX,
            UInt160::zero(),
            message,
            Witness::empty(),
        );

        if let Err(e) = payload.validate() {
            return Err(ConsensusError::Network(format!(
                "Consensus payload validation failed: {}",
                e
            )));
        }

        let network_msg = NetworkMessage::new(ProtocolMessage::Extensible { payload });
        self.p2p_node
            .broadcast_message(network_msg)
            .await
            .map_err(|e| ConsensusError::Network(format!("Failed to broadcast: {}", e)))
    }

    async fn send_consensus_message(&self, peer_id: &str, message: Vec<u8>) -> ConsensusResult<()> {
        let payload = ExtensiblePayload::new(
            "dBFT".to_string(),
            0,
            u32::MAX,
            UInt160::zero(),
            message,
            Witness::empty(),
        );

        if let Err(e) = payload.validate() {
            return Err(ConsensusError::Network(format!(
                "Consensus payload validation failed: {}",
                e
            )));
        }

        let network_msg = NetworkMessage::new(ProtocolMessage::Extensible { payload });

        if let Ok(addr) = peer_id.parse() {
            self.p2p_node
                .send_message_to_peer(addr, network_msg)
                .await
                .map_err(|e| ConsensusError::Network(format!("Failed to send message: {}", e)))
        } else {
            Err(ConsensusError::Network(format!(
                "Invalid peer ID: {}",
                peer_id
            )))
        }
    }

    async fn get_connected_peers(&self) -> ConsensusResult<Vec<String>> {
        let peers = self.p2p_node.get_connected_peer_addresses().await;
        Ok(peers.into_iter().map(|p| p.to_string()).collect())
    }

    async fn is_connected(&self) -> bool {
        self.p2p_node.get_connected_peer_addresses().await.len() > 0
    }
}

/// Unified mempool adapter that shares the same mempool between consensus and ledger
pub struct UnifiedMempool {
    inner: Arc<RwLock<neo_ledger::MemoryPool>>,
}

impl UnifiedMempool {
    pub fn new(mempool: Arc<RwLock<neo_ledger::MemoryPool>>) -> Self {
        Self { inner: mempool }
    }
}

#[async_trait]
impl neo_consensus::MempoolService for UnifiedMempool {
    async fn get_verified_transactions(&self, count: usize) -> Vec<Transaction> {
        let mempool = self.inner.read().await;
        mempool.get_sorted_transactions(Some(count))
    }

    async fn contains_transaction(&self, hash: &UInt256) -> bool {
        let mempool = self.inner.read().await;
        mempool.contains(hash)
    }

    async fn add_transaction(&self, tx: Transaction) -> ConsensusResult<()> {
        let mempool = self.inner.read().await;
        mempool
            .try_add(tx, false)
            .map(|_| ())
            .map_err(|e| ConsensusError::Generic(format!("Failed to add transaction: {}", e)))
    }

    async fn remove_transaction(&self, hash: &UInt256) -> ConsensusResult<()> {
        let mempool = self.inner.read().await;
        mempool
            .try_remove(hash)
            .map(|_| ())
            .map_err(|e| ConsensusError::Generic(format!("Failed to remove transaction: {}", e)))
    }

    async fn get_transaction_count(&self) -> usize {
        let mempool = self.inner.read().await;
        mempool.get_all_transactions().len()
    }

    async fn clear(&self) -> ConsensusResult<()> {
        let mempool = self.inner.read().await;
        mempool
            .clear_expired_transactions()
            .map(|_| ())
            .map_err(|e| ConsensusError::Generic(format!("Failed to clear mempool: {}", e)))
    }
}
