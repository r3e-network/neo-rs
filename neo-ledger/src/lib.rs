//! Ledger persistence and block/transaction validation subsystem.

pub mod block_cache_budget;
pub mod blockchain;
pub mod events;
pub mod hydration;
pub mod ledger_context;
pub mod persistence;
pub mod services;
pub mod validation;

// Re-export key types at the crate root
pub use block_cache_budget::{
    BLOCK_CACHE_ENTRY_OVERHEAD, BlockCacheBudget, BlockCacheReservation,
    DEFAULT_BLOCK_CACHE_BUDGET_BYTES, block_cache_cost, execution_block_cache_cost,
    payload_block_cache_cost,
};
pub use ledger_context::LedgerContext;
pub use services::LedgerService;
pub use validation::{HeaderCache, HeaderCacheIter};

// Re-export the ledger-owned hydration surface
pub use hydration::{
    LedgerHydrationError, LedgerHydrationResult, PointerField, hydrate_ledger,
    hydrate_ledger_tip_for_checkpoint_replay,
};

// Re-export blockchain types
pub use blockchain::admission::{
    BlockAdmissionPort, PersistenceAdmission, admit_block_for_persistence,
};
pub use blockchain::cache_policy::{
    BlockCachePolicy, MAX_BLOCK_CACHE_SIZE, MAX_UNVERIFIED_CACHE_BYTES, MAX_UNVERIFIED_CACHE_SIZE,
    MAX_UNVERIFIED_CANDIDATES_PER_HEIGHT, PersistedBodyCache, RetainedBlock,
};
pub use blockchain::types::{
    BlockchainCommand, BlockchainHandle, FillCompleted, FillMemoryPool, Import, ImportCompleted,
    ImportDisposition, InventoryPayload, PersistCompleted, PersistFailed, PreverifyCompleted,
    RelayResult, Reverify, ReverifyItem, UnverifiedBlocksList, classify_import_block,
};

// Re-export persistence types
pub use persistence::{
    ApplicationEnginePort, CommitContext, CommitParticipant, EngineFactory, EvidenceSink,
    ExecutionReceipt, FaultCategory, FaultObservation, LedgerError, LedgerResult,
    MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK, NativeCacheHandle, NativeEnginePort, PersistOptions,
    PersistReceipt, PersistenceProfiler, PersistenceStage, RuntimeCacheSink, SeededNativeContracts,
    SnapshotStore, SpeculativeExecution, StageContext, StoreTransactionPort,
    TransactionLoopContext, TransactionLoopOutcome, TransactionLoopStrategy, execute_serially,
    persist_block, persist_block_profiled, persist_block_with_loop_strategy,
};

// Re-export validation types
pub use validation::{
    BlockLike, BlockValidationError, HEADER_VERIFY_GAS, LedgerWitnessHashResolver, MAX_HEADERS,
    MAX_TIMESTAMP_DRIFT_MS, MIN_TIMESTAMP_MS, StandardWitnessVerification,
    TransactionVerificationContext, VerifiableExt, attribute_network_fee,
    block_script_hashes_for_verifying, calculate_block_network_fee,
    header_script_hashes_for_verifying, oracle_fixed_script, parse_single_signature_contract,
    parse_single_signature_invocation, transaction_script_hashes_for_verifying,
    transaction_sign_data, validate_block_size, validate_block_size_raw, validate_block_version,
    validate_merkle_root, validate_no_duplicate_transactions, validate_primary_index,
    validate_timestamp_bounds, validate_timestamp_progression, validate_transaction_count,
    validate_transaction_count_raw, validate_wire_transaction_count_raw, validate_witness_scripts,
    verify_attribute, verify_block, verify_block_integrity, verify_block_proposal_policy,
    verify_block_with_cache, verify_execution_block_integrity, verify_header,
    verify_header_with_cache, verify_header_witness_against_hash, verify_standard_witness,
    verify_transaction, verify_transaction_state_dependent,
    verify_transaction_state_dependent_at_height, verify_transaction_state_independent,
    verify_transaction_witness, verify_witness, verify_witness_multi_signature,
    verify_witness_signature, verify_witnesses,
};

// Re-export commonly used types from dependencies
pub use neo_primitives::VerifyResult;
pub use neo_protocol::block::Block;

// Re-export genesis block creation from neo-native-contracts
pub use neo_native_contracts::create_genesis_block;

// Re-export ApplicationExecuted from neo-protocol
pub use neo_protocol::execution::ApplicationExecuted;
