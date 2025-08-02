//! Mock implementations for integration tests
//! 
//! This module provides mock types and implementations to allow integration tests
//! to compile. These mocks should be replaced with actual implementations as they
//! become available.

#![allow(dead_code, unused_variables)]

use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use std::time::Duration;
use std::net::SocketAddr;
use std::collections::HashMap;

// Re-export actual types that exist
pub use neo_core::{UInt160, UInt256};
pub use neo_config::NetworkType;

// Mock Transaction
#[derive(Clone)]
pub struct Transaction {
    pub version: u8,
    pub nonce: u32,
    pub system_fee: i64,
    pub network_fee: i64,
    pub valid_until_block: u32,
    pub signers: Vec<Signer>,
    pub attributes: Vec<TransactionAttribute>,
    pub script: Vec<u8>,
    pub witnesses: Vec<Witness>,
}

impl Transaction {
    pub fn hash(&self) -> Result<UInt256, Box<dyn std::error::Error>> {
        Ok(UInt256::default())
    }
}

#[derive(Clone)]
pub struct TransactionAttribute;

// Mock Signer if not available in neo_core
#[derive(Clone)]
pub struct Signer {
    pub account: UInt160,
    pub scopes: WitnessScope,
    pub allowed_contracts: Vec<UInt160>,
    pub allowed_groups: Vec<Vec<u8>>,
    pub rules: Vec<WitnessRule>,
}

#[derive(Clone)]
pub enum WitnessScope {
    None = 0x00,
    CalledByEntry = 0x01,
    CustomContracts = 0x10,
    CustomGroups = 0x20,
    WitnessRules = 0x40,
    Global = 0xFF,
}

#[derive(Clone)]
pub struct WitnessRule;

// Mock Witness
#[derive(Clone)]
pub struct Witness {
    pub invocation_script: Vec<u8>,
    pub verification_script: Vec<u8>,
}

impl Default for Witness {
    fn default() -> Self {
        Self {
            invocation_script: vec![0x00; 64],
            verification_script: vec![0x51], // PUSH1
        }
    }
}

// Mock Node implementation
pub mod node {
    use super::*;
    
    #[derive(Clone)]
    pub struct Node {
        config: NodeConfig,
    }
    
    #[derive(Clone)]
    pub struct NodeConfig {
        pub network: crate::test_mocks::network::NetworkConfig,
        pub consensus: crate::test_mocks::consensus::ConsensusConfig,
        pub ledger: neo_config::LedgerConfig,
        pub rpc: neo_config::RpcServerConfig,
        pub data_path: String,
        pub network_type: NetworkType,
    }
    
    impl Node {
        pub async fn new(config: NodeConfig) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(Self { config })
        }
        
        pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
        
        pub async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
        
        pub async fn add_peer(&mut self, address: String) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
        
        pub async fn connect_peer(&self, address: &str) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
    }
}

// Mock Network types
pub mod network {
    use super::*;
    
    #[derive(Clone)]
    pub struct NetworkConfig {
        pub enabled: bool,
        pub port: u16,
        pub max_outbound_connections: usize,
        pub max_inbound_connections: usize,
        pub connection_timeout_secs: u64,
        pub seed_nodes: Vec<SocketAddr>,
        pub user_agent: String,
        pub protocol_version: u32,
        pub websocket_enabled: bool,
        pub websocket_port: u16,
    }
    
    pub mod p2p {
        use super::*;
        
        #[derive(Clone)]
        pub struct PeerConfig {
            pub max_peers: usize,
            pub connection_timeout: Duration,
            pub handshake_timeout: Duration,
            pub message_timeout: Duration,
            pub ping_interval: Duration,
            pub ping_timeout: Duration,
        }
        
        #[derive(Clone)]
        pub struct Node {
            config: NodeConfig,
        }
        
        #[derive(Clone)]
        pub struct NodeConfig {
            pub network: NetworkConfig,
            pub data_path: String,
            pub network_type: NetworkType,
            pub max_peers: usize,
            pub connection_timeout: Duration,
            pub handshake_timeout: Duration,
            pub message_timeout: Duration,
            pub ping_interval: Duration,
            pub ping_timeout: Duration,
        }
        
        impl Node {
            pub async fn new(config: NodeConfig) -> Result<Self, Box<dyn std::error::Error>> {
                Ok(Self { config })
            }
            
            pub async fn start(&self) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }
            
            pub async fn add_peer(&mut self, address: String) -> Result<(), Box<dyn std::error::Error>> {
                Ok(())
            }
        }
    }
    
    pub mod messages {
        use super::*;
        
        #[derive(Clone, Debug)]
        pub struct NetworkMessage {
            pub payload: ProtocolMessage,
        }
        
        #[derive(Clone, Debug)]
        pub enum ProtocolMessage {
            Version(VersionMessage),
            GetHeaders,
            Headers,
            GetBlocks,
            Block,
        }
        
        #[derive(Clone, Debug)]
        pub struct VersionMessage {
            pub version: u32,
            pub timestamp: u64,
            pub nonce: u32,
            pub user_agent: String,
            pub start_height: u32,
            pub relay: bool,
        }
    }
    
    pub mod sync {
        use super::*;
        
        pub struct SyncManager {
            blockchain: Arc<crate::test_mocks::ledger::Blockchain>,
            strategy: SyncStrategy,
        }
        
        #[derive(Clone)]
        pub enum SyncStrategy {
            FastSync,
            HeadersFirst,
            ParallelDownload { max_parallel: usize },
            CheckpointSync,
        }
        
        #[derive(Clone, PartialEq)]
        pub enum SyncState {
            Idle,
            Synchronizing,
            Synchronized,
            Failed(String),
        }
        
        impl SyncManager {
            pub fn new(blockchain: Arc<crate::test_mocks::ledger::Blockchain>, strategy: SyncStrategy) -> Self {
                Self { blockchain, strategy }
            }
            
            pub fn new_with_checkpoint(
                blockchain: Arc<crate::test_mocks::ledger::Blockchain>,
                strategy: SyncStrategy,
                _height: u32,
                _hash: UInt256,
            ) -> Self {
                Self { blockchain, strategy }
            }
            
            pub async fn add_peer(&self, _peer: MockPeer) {}
            pub async fn start_sync(&self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
            pub async fn get_sync_state(&self) -> SyncState { SyncState::Synchronized }
            pub async fn sync_headers(&self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
            pub async fn get_header_height(&self) -> u32 { 0 }
            pub async fn get_header(&self, _height: u32) -> Option<crate::test_mocks::ledger::BlockHeader> { None }
            pub async fn get_peer_statistics(&self) -> HashMap<u64, PeerStats> { HashMap::new() }
            pub async fn get_validation_statistics(&self) -> ValidationStats { ValidationStats::default() }
            pub async fn get_statistics(&self) -> SyncStats { SyncStats::default() }
        }
        
        #[derive(Default)]
        pub struct PeerStats {
            pub blocks_downloaded: u32,
        }
        
        #[derive(Default)]
        pub struct ValidationStats {
            pub full_validations: u32,
        }
        
        #[derive(Default)]
        pub struct SyncStats {
            pub blocks_processed: u32,
        }
    }
    
    pub mod peer_manager {
        pub struct PeerManager;
    }
    
    pub mod server {
        pub struct NetworkServer;
    }
}

// Mock Consensus types
pub mod consensus {
    use super::*;
    
    #[derive(Clone)]
    pub struct ConsensusConfig {
        pub enabled: bool,
        pub validator_index: Option<usize>,
        pub validator_count: usize,
        pub view_timeout_ms: u64,
        pub block_time_ms: u64,
    }
    
    #[derive(Clone)]
    pub struct ValidatorConfig {
        pub public_key: String,
        pub voting_power: u64,
    }
    
    pub struct DbftEngine {
        config: DbftConfig,
    }
    
    #[derive(Clone)]
    pub struct DbftConfig {
        pub validator_count: usize,
        pub view_timeout: Duration,
        pub max_block_size: usize,
        pub max_transactions_per_block: usize,
    }
    
    #[derive(Clone)]
    pub struct ConsensusContext;
    
    #[derive(Clone, PartialEq)]
    pub enum ConsensusPhase {
        RequestSent,
        CommitSent,
    }
    
    pub mod messages {
        #[derive(Clone)]
        pub struct ConsensusMessage;
        #[derive(Clone)]
        pub struct PrepareRequest;
        #[derive(Clone)]
        pub struct PrepareResponse;
        #[derive(Clone)]
        pub struct Commit;
        #[derive(Clone)]
        pub struct ChangeView;
    }
    
    impl DbftEngine {
        pub async fn new(
            config: DbftConfig,
            _validator_id: usize,
            _blockchain: Arc<crate::test_mocks::ledger::Blockchain>,
            _mempool: Arc<RwLock<crate::test_mocks::ledger::MemoryPool>>,
            _tx: mpsc::UnboundedSender<messages::ConsensusMessage>,
        ) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(Self { config })
        }
        
        pub async fn start_consensus_round(&self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
        pub async fn handle_consensus_message(&self, _msg: messages::ConsensusMessage) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
        pub async fn get_last_agreed_block(&self) -> Option<crate::test_mocks::ledger::Block> { None }
        pub async fn start_view_timer(&self) {}
        pub async fn get_current_view(&self) -> u32 { 0 }
        pub async fn get_consensus_context(&self) -> ConsensusContext { ConsensusContext }
        pub async fn create_recovery_request(&self) -> RecoveryRequest { RecoveryRequest }
        pub async fn create_recovery_message(&self, _req: &RecoveryRequest) -> RecoveryMessage { RecoveryMessage }
        pub async fn handle_recovery_message(&self, _msg: RecoveryMessage) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
        pub async fn get_mempool(&self) -> Arc<RwLock<crate::test_mocks::ledger::MemoryPool>> { 
            Arc::new(RwLock::new(crate::test_mocks::ledger::MemoryPool::new()))
        }
    }
    
    pub struct RecoveryRequest;
    pub struct RecoveryMessage;
    
    impl ConsensusContext {
        pub async fn set_phase(&self, _phase: ConsensusPhase) {}
        pub async fn add_prepare_response(&self, _validator: u16) {}
        pub async fn get_phase(&self) -> ConsensusPhase { ConsensusPhase::RequestSent }
        pub async fn get_prepare_response_count(&self) -> usize { 0 }
        pub async fn save_state(&self) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
    }
}

// Mock Ledger types  
pub mod ledger {
    use super::*;
    
    pub struct Blockchain {
        height: Arc<RwLock<u32>>,
    }
    
    #[derive(Clone)]
    pub struct Block {
        pub header: BlockHeader,
        pub transactions: Vec<Transaction>,
    }
    
    #[derive(Clone)]
    pub struct BlockHeader {
        pub version: u32,
        pub prev_hash: UInt256,
        pub merkle_root: UInt256,
        pub timestamp: u64,
        pub index: u32,
        pub primary_index: u8,
        pub next_consensus: UInt160,
        pub witness: crate::test_mocks::MockWitness,
    }
    
    pub struct MemoryPool;
    
    impl Blockchain {
        pub async fn new(_network: NetworkType, _path: &str) -> Result<Self, Box<dyn std::error::Error>> {
            Ok(Self {
                height: Arc::new(RwLock::new(0)),
            })
        }
        
        pub async fn get_height(&self) -> Result<u32, Box<dyn std::error::Error>> {
            Ok(*self.height.read().await)
        }
        
        pub async fn get_block_by_index(&self, index: u32) -> Result<Option<Block>, Box<dyn std::error::Error>> {
            Ok(Some(Block {
                header: BlockHeader {
                    version: 0,
                    prev_hash: UInt256::default(),
                    merkle_root: UInt256::default(),
                    timestamp: 0,
                    index,
                    primary_index: 0,
                    next_consensus: UInt160::default(),
                    witness: crate::test_mocks::MockWitness::default(),
                },
                transactions: vec![],
            }))
        }
        
        pub async fn get_block_by_hash(&self, _hash: &UInt256) -> Result<Option<Block>, Box<dyn std::error::Error>> {
            Ok(None)
        }
        
        pub async fn add_block(&self, _block: Block) -> Result<(), Box<dyn std::error::Error>> {
            let mut height = self.height.write().await;
            *height += 1;
            Ok(())
        }
        
        pub async fn get_current_block(&self) -> Result<Block, Box<dyn std::error::Error>> {
            Ok(Block {
                header: BlockHeader {
                    version: 0,
                    prev_hash: UInt256::default(),
                    merkle_root: UInt256::default(),
                    timestamp: 0,
                    index: 0,
                    primary_index: 0,
                    next_consensus: UInt160::default(),
                    witness: crate::test_mocks::MockWitness::default(),
                },
                transactions: vec![],
            })
        }
        
        pub async fn verify_transaction(&self, _tx: &Transaction) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
        
        pub async fn set_balance(&self, _account: &UInt160, _token: &UInt160, _amount: i64) {}
        pub async fn get_balance(&self, _account: &UInt160, _token: &UInt160) -> i64 { 0 }
        
        pub async fn get_transaction_receipt(&self, _hash: &UInt256) -> Result<Option<TransactionReceipt>, Box<dyn std::error::Error>> {
            Ok(Some(TransactionReceipt {
                vm_state: VmState::Halt,
            }))
        }
    }
    
    impl Block {
        pub fn hash(&self) -> UInt256 {
            UInt256::default()
        }
    }
    
    impl MemoryPool {
        pub fn new() -> Self { Self }
        pub async fn add_transaction(&self, _tx: Transaction) -> Result<(), Box<dyn std::error::Error>> { Ok(()) }
        pub fn size(&self) -> usize { 0 }
    }
    
    pub struct TransactionReceipt {
        pub vm_state: VmState,
    }
    
    pub enum VmState {
        Halt,
        Fault,
    }
    
    impl VmState {
        pub fn is_success(&self) -> bool {
            matches!(self, VmState::Halt)
        }
    }
}

// Mock RPC types
pub mod rpc_client {
    use super::*;
    
    pub struct RpcClient {
        url: String,
    }
    
    impl RpcClient {
        pub fn new(url: &str) -> Self {
            Self { url: url.to_string() }
        }
        
        pub async fn send_raw_transaction(&self, _tx: &Transaction) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
        
        pub async fn get_block_count(&self) -> Result<u32, Box<dyn std::error::Error>> {
            Ok(100)
        }
        
        pub async fn get_block(&self, _height: u32) -> Result<crate::test_mocks::ledger::Block, Box<dyn std::error::Error>> {
            Ok(crate::test_mocks::ledger::Block {
                header: crate::test_mocks::ledger::BlockHeader {
                    version: 0,
                    prev_hash: UInt256::default(),
                    merkle_root: UInt256::default(),
                    timestamp: 0,
                    index: 0,
                    primary_index: 0,
                    next_consensus: UInt160::default(),
                    witness: crate::test_mocks::MockWitness::default(),
                },
                transactions: vec![],
            })
        }
        
        pub async fn get_raw_mempool(&self) -> Result<Vec<UInt256>, Box<dyn std::error::Error>> {
            Ok(vec![])
        }
    }
}

// Mock VM types
pub mod vm {
    use super::*;
    
    pub type Script = Vec<u8>;
    
    #[derive(Clone)]
    pub enum StackItem {
        Integer(i64),
        ByteArray(Vec<u8>),
        Boolean(bool),
    }
    
    pub struct ExecutionEngine;
    
    #[repr(u8)]
    #[derive(Clone, Copy)]
    pub enum OpCode {
        PUSH0 = 0x00,
        PUSH1 = 0x51,
        PUSH2 = 0x52,
        PUSH3 = 0x53,
        PUSH10 = 0x5A,
        PUSHINT32 = 0x02,
        PUSHINT64 = 0x04,
        PUSHDATA1 = 0x0C,
        PUSHDATA2 = 0x0D,
        PUSHNULL = 0x08,
        ADD = 0x93,
        MUL = 0x95,
        DIV = 0x96,
        LT = 0x9F,
        ASSERT = 0x38,
        PACK = 0xC1,
        SHA256 = 0xA8,
        SYSCALL = 0x41,
        RET = 0x40,
        JMPIF = 0x63,
        LDARG0 = 0x00,
        LDARG1 = 0x01,
    }
    
    impl OpCode {
        pub fn to_u8(self) -> u8 {
            self as u8
        }
    }
}

// Mock Smart Contract types
pub mod smart_contract {
    use super::*;
    
    pub struct ApplicationEngine {
        gas_consumed: u64,
    }
    
    pub struct Contract;
    pub struct ContractState;
    pub struct NefFile {
        pub magic: [u8; 4],
        pub compiler: String,
        pub version: String,
        pub script: Vec<u8>,
        pub checksum: u32,
    }
    
    pub struct ContractManifest {
        pub name: String,
        pub groups: Vec<String>,
        pub features: ContractFeatures,
        pub supported_standards: Vec<String>,
        pub abi: ContractAbi,
        pub permissions: Vec<String>,
        pub trusts: Vec<String>,
        pub extra: Option<String>,
    }
    
    #[derive(Default)]
    pub struct ContractFeatures;
    #[derive(Default)]
    pub struct ContractAbi;
    
    #[derive(Clone, Copy)]
    pub enum TriggerType {
        Application,
        Verification,
    }
    
    impl ApplicationEngine {
        pub fn new(
            _trigger: TriggerType,
            _tx: &Transaction,
            _blockchain: Arc<crate::test_mocks::ledger::Blockchain>,
            _snapshot: Option<()>,
            gas_limit: i64,
        ) -> Self {
            Self { gas_consumed: 0 }
        }
        
        pub async fn execute(&self) -> Result<(), Box<dyn std::error::Error>> {
            Ok(())
        }
        
        pub async fn get_balance(&self, _account: &UInt160, _token: &UInt160) -> i64 {
            0
        }
        
        pub fn get_gas_consumed(&self) -> u64 {
            self.gas_consumed
        }
        
        pub fn get_deployed_contract_hash(&self) -> Option<UInt160> {
            Some(UInt160::default())
        }
        
        pub fn get_result_stack(&self) -> Vec<crate::test_mocks::vm::StackItem> {
            vec![crate::test_mocks::vm::StackItem::Integer(30)]
        }
    }
    
    impl NefFile {
        pub fn to_bytes(&self) -> Vec<u8> {
            vec![]
        }
    }
}

// Mock witness type
#[derive(Clone, Default)]
pub struct MockWitness {
    pub invocation_script: Vec<u8>,
    pub verification_script: Vec<u8>,
}

// Mock peer for sync tests
#[derive(Clone)]
pub struct MockPeer {
    pub id: u64,
    pub blockchain: Arc<ledger::Blockchain>,
    pub response_delay: Duration,
}

// Helper trait implementations
impl std::str::FromStr for UInt160 {
    type Err = Box<dyn std::error::Error>;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Simple mock implementation
        Ok(UInt160::default())
    }
}

impl std::str::FromStr for UInt256 {
    type Err = Box<dyn std::error::Error>;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Simple mock implementation
        Ok(UInt256::default())
    }
}