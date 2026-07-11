//! Error types for the Neo indexer service.

use neo_error::CoreError;
use neo_primitives::UInt256;
use neo_storage::StorageError;
use thiserror::Error;

/// Result alias used by the indexer.
pub type IndexerResult<T> = Result<T, IndexerError>;

/// Errors returned while building or mutating indexes.
#[derive(Debug, Error)]
pub enum IndexerError {
    /// A prepared single-block command completed without a block record.
    #[error("prepared indexer command produced no block record")]
    MissingPreparedBlockResult,

    /// The block header hash could not be computed.
    #[error("failed to hash block")]
    BlockHash {
        /// Source serialization/hash error.
        #[source]
        source: CoreError,
    },

    /// A transaction hash could not be computed.
    #[error("failed to hash transaction at index {index}")]
    TransactionHash {
        /// Transaction position inside the block.
        index: u32,
        /// Source serialization/hash error.
        #[source]
        source: CoreError,
    },

    /// The block contains more transactions than the index model can address.
    #[error("block contains too many transactions: {count}")]
    TooManyTransactions {
        /// Transaction count observed in the block.
        count: usize,
    },

    /// A block contains more application executions than the index model can
    /// address.
    #[error("block contains too many application executions: {count}")]
    TooManyExecutions {
        /// Execution count observed for the block.
        count: usize,
    },

    /// An execution contains more notifications than the index model can
    /// address.
    #[error("execution {execution_index} contains too many notifications: {count}")]
    TooManyNotifications {
        /// Execution position inside the block.
        execution_index: u32,
        /// Notification count observed for the execution.
        count: usize,
    },

    /// A notification contains more state items than the index model can
    /// address.
    #[error(
        "notification {notification_index} in execution {execution_index} contains too many state items: {count}"
    )]
    TooManyNotificationStateItems {
        /// Execution position inside the block.
        execution_index: u32,
        /// Notification position inside the execution.
        notification_index: u32,
        /// State item count observed for the notification.
        count: usize,
    },

    /// A notification state payload could not be rendered as Neo JSON-RPC
    /// stack item JSON.
    #[error(
        "failed to render notification {notification_index} in execution {execution_index} state"
    )]
    NotificationStateJson {
        /// Execution position inside the block.
        execution_index: u32,
        /// Notification position inside the execution.
        notification_index: u32,
        /// Source VM rendering error.
        #[source]
        source: neo_vm::VmError,
    },

    /// A transaction hash could not be computed for an execution record.
    #[error("failed to hash transaction for execution {execution_index}")]
    ExecutionTransactionHash {
        /// Execution position inside the block.
        execution_index: u32,
        /// Source serialization/hash error.
        #[source]
        source: CoreError,
    },

    /// An application execution references a transaction that is not part of
    /// the block being indexed.
    #[error("execution {execution_index} references transaction {hash} outside the indexed block")]
    ExecutionTransactionNotInBlock {
        /// Execution position inside the block.
        execution_index: u32,
        /// Transaction hash carried by the execution.
        hash: UInt256,
    },

    /// A block contains the same transaction hash more than once.
    #[error("duplicate transaction in block: {hash}")]
    DuplicateTransaction {
        /// Duplicate transaction hash.
        hash: UInt256,
    },

    /// A persisted transaction record uses a transaction index outside its
    /// block's declared transaction count.
    #[error(
        "transaction {hash} records index {transaction_index}, but block {block_hash} has {transaction_count} transactions"
    )]
    TransactionIndexOutOfBounds {
        /// Transaction hash.
        hash: UInt256,
        /// Referenced block hash.
        block_hash: UInt256,
        /// Transaction index stored in the snapshot.
        transaction_index: u32,
        /// Transaction count stored on the block record.
        transaction_count: u32,
    },

    /// A persisted snapshot contains two transaction records for the same
    /// `(block, transaction_index)` coordinate.
    #[error(
        "duplicate transaction position in snapshot: block {block_hash} at height {block_height}, index {transaction_index}"
    )]
    DuplicateTransactionPosition {
        /// Referenced block hash.
        block_hash: UInt256,
        /// Referenced block height.
        block_height: u32,
        /// Duplicate transaction position inside the block.
        transaction_index: u32,
    },

    /// A persisted block record's declared transaction count does not match the
    /// number of transaction records attached to that block.
    #[error(
        "block {block_hash} at height {block_height} records {expected} transactions, but snapshot contains {actual}"
    )]
    TransactionCountMismatch {
        /// Referenced block hash.
        block_hash: UInt256,
        /// Referenced block height.
        block_height: u32,
        /// Transaction count stored on the block record.
        expected: u32,
        /// Number of transaction records found for the block.
        actual: usize,
    },

    /// A block's transaction list references a transaction absent from the
    /// transaction table while rebuilding a snapshot.
    #[error("block {block_hash} at height {block_height} references missing transaction {tx_hash}")]
    MissingBlockTransactionRecord {
        /// Missing transaction hash.
        tx_hash: UInt256,
        /// Block hash whose transaction list referenced the transaction.
        block_hash: UInt256,
        /// Block height whose transaction list referenced the transaction.
        block_height: u32,
    },

    /// A persisted snapshot uses a schema version this crate does not support.
    #[error("unsupported indexer snapshot version {version}")]
    UnsupportedSnapshotVersion {
        /// Snapshot version read from disk.
        version: u32,
    },

    /// A persisted snapshot contains the same block hash more than once.
    #[error("duplicate block hash in indexer snapshot: {hash}")]
    DuplicateBlockHash {
        /// Duplicate block hash.
        hash: UInt256,
    },

    /// A persisted snapshot contains more than one block for the same height.
    #[error("duplicate block height in indexer snapshot: {height}")]
    DuplicateBlockHeight {
        /// Duplicate block height.
        height: u32,
    },

    /// The height index references a block hash that is absent from the block
    /// table while rebuilding a snapshot.
    #[error("height index entry {height} points to missing block {block_hash}")]
    MissingHeightIndexBlock {
        /// Height recorded in the height index.
        height: u32,
        /// Block hash referenced by the height index.
        block_hash: UInt256,
    },

    /// A persisted transaction record points to a block absent from the
    /// snapshot.
    #[error("transaction {hash} points to missing block {block_hash}")]
    MissingTransactionBlock {
        /// Transaction hash.
        hash: UInt256,
        /// Referenced block hash.
        block_hash: UInt256,
    },

    /// A persisted transaction record disagrees with its block record's height.
    #[error(
        "transaction {hash} records block height {transaction_height}, \
         but block {block_hash} is indexed at height {block_height}"
    )]
    TransactionBlockHeightMismatch {
        /// Transaction hash.
        hash: UInt256,
        /// Referenced block hash.
        block_hash: UInt256,
        /// Height stored in the transaction record.
        transaction_height: u32,
        /// Height stored in the block record.
        block_height: u32,
    },

    /// A persisted notification record points to a block absent from the
    /// snapshot.
    #[error(
        "notification {notification_index} in execution {execution_index} points to missing block {block_hash}"
    )]
    MissingNotificationBlock {
        /// Referenced block hash.
        block_hash: UInt256,
        /// Height stored in the notification record.
        block_height: u32,
        /// Execution position inside the block.
        execution_index: u32,
        /// Notification position inside the execution.
        notification_index: u32,
    },

    /// A persisted notification record disagrees with its block record's height.
    #[error(
        "notification {notification_index} in execution {execution_index} records block height {notification_height}, \
         but block {block_hash} is indexed at height {block_height}"
    )]
    NotificationBlockHeightMismatch {
        /// Referenced block hash.
        block_hash: UInt256,
        /// Height stored in the notification record.
        notification_height: u32,
        /// Height stored in the block record.
        block_height: u32,
        /// Execution position inside the block.
        execution_index: u32,
        /// Notification position inside the execution.
        notification_index: u32,
    },

    /// A persisted notification record references a transaction absent from the
    /// snapshot.
    #[error(
        "notification {notification_index} in execution {execution_index} points to missing transaction {tx_hash}"
    )]
    MissingNotificationTransaction {
        /// Referenced transaction hash.
        tx_hash: UInt256,
        /// Referenced block hash.
        block_hash: UInt256,
        /// Execution position inside the block.
        execution_index: u32,
        /// Notification position inside the execution.
        notification_index: u32,
    },

    /// A persisted notification record references a transaction in a different
    /// block.
    #[error(
        "notification {notification_index} in execution {execution_index} points to transaction {tx_hash} in block {transaction_block_hash}, \
         but notification block is {block_hash}"
    )]
    NotificationTransactionBlockMismatch {
        /// Referenced transaction hash.
        tx_hash: UInt256,
        /// Transaction's indexed block hash.
        transaction_block_hash: UInt256,
        /// Notification's referenced block hash.
        block_hash: UInt256,
        /// Execution position inside the block.
        execution_index: u32,
        /// Notification position inside the execution.
        notification_index: u32,
    },

    /// A persisted snapshot contains duplicate notification coordinates for a
    /// block.
    #[error(
        "duplicate notification coordinate in snapshot: block {block_hash}, execution {execution_index}, notification {notification_index}"
    )]
    DuplicateNotification {
        /// Referenced block hash.
        block_hash: UInt256,
        /// Execution position inside the block.
        execution_index: u32,
        /// Notification position inside the execution.
        notification_index: u32,
    },

    /// A service-store record could not be decoded.
    #[error("failed to decode indexer service-store record at key {key:?}")]
    StoreRecordDecode {
        /// Raw store key.
        key: Vec<u8>,
        /// Source JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// The synchronized projection checkpoint is absent from its service
    /// store.
    #[error("indexer service store is missing checkpoint block {height}")]
    MissingCheckpointBlock {
        /// Expected checkpoint height.
        height: u32,
    },

    /// The synchronized projection checkpoint disagrees with its service
    /// store record.
    #[error(
        "indexer checkpoint block {height} hash mismatch: projection {expected}, store {actual}"
    )]
    CheckpointBlockMismatch {
        /// Checkpoint height.
        height: u32,
        /// Hash held by the synchronized projection.
        expected: UInt256,
        /// Hash decoded from the service store.
        actual: UInt256,
    },

    /// A service-store record could not be encoded.
    #[error("failed to encode indexer service-store record at key {key:?}")]
    StoreRecordEncode {
        /// Raw store key.
        key: Vec<u8>,
        /// Source JSON error.
        #[source]
        source: serde_json::Error,
    },

    /// The service-store schema version is not supported by this crate.
    #[error("unsupported indexer service-store schema version {version}")]
    UnsupportedStoreSchemaVersion {
        /// Version read from the store.
        version: String,
    },

    /// The removed whole-snapshot service-store format is still present.
    #[error(
        "legacy indexer service-store snapshot is unsupported; remove the indexer store and rebuild its derived projection"
    )]
    LegacyStoreSnapshotUnsupported,

    /// Service-store records could not be written.
    #[error("failed to write indexer records to service store")]
    StoreRecordWrite {
        /// Source storage error.
        #[source]
        source: StorageError,
    },

    /// A service-store snapshot handle was unexpectedly shared while writing.
    #[error("failed to write indexer records: service-store snapshot is still shared")]
    StoreSnapshotShared,
}
