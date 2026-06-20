//! Shared support types for workspace integration tests.

/// Lightweight mempool test doubles used by integration tests.
pub mod mempool {
    /// Lightweight fee-policy placeholder used by integration tests.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct FeePolicy;

    /// Test mempool configuration facade.
    #[derive(Debug, Clone)]
    pub struct MempoolConfig {
        /// Maximum transactions accepted by the test mempool.
        pub max_transactions: usize,
        /// Maximum transactions accepted from one sender.
        pub max_per_sender: usize,
        /// Placeholder fee policy used by legacy tests.
        pub fee_policy: FeePolicy,
        /// Whether replacement-by-fee behavior is enabled.
        pub enable_replacement: bool,
        /// Required fee increase percentage for replacement.
        pub replacement_fee_increase: u32,
    }

    impl Default for MempoolConfig {
        fn default() -> Self {
            Self {
                max_transactions: 50_000,
                max_per_sender: 512,
                fee_policy: FeePolicy,
                enable_replacement: true,
                replacement_fee_increase: 10,
            }
        }
    }

    /// Minimal empty-pool facade for legacy integration tests.
    #[derive(Debug, Clone, Default)]
    pub struct Mempool {
        _config: MempoolConfig,
    }

    impl Mempool {
        /// Create an empty test mempool with default configuration.
        pub fn new() -> Self {
            Self::with_config(MempoolConfig::default())
        }

        /// Create an empty test mempool with explicit configuration.
        pub fn with_config(config: MempoolConfig) -> Self {
            Self { _config: config }
        }

        /// Return the number of queued test transactions.
        pub fn len(&self) -> usize {
            0
        }

        /// Return whether the test mempool has no queued transactions.
        pub fn is_empty(&self) -> bool {
            true
        }

        /// Return the top test transactions by priority.
        pub fn get_top(&self, _count: usize) -> Vec<()> {
            Vec::new()
        }
    }
}

/// Lightweight state test doubles used by integration tests.
pub mod state {
    use neo_crypto::Crypto;
    use neo_primitives::{UInt160, UInt256};
    use std::collections::BTreeMap;

    /// Storage key used by the in-memory world-state test double.
    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct StorageKey {
        /// Contract script hash owning this storage entry.
        pub contract: UInt160,
        /// Raw contract-local storage key bytes.
        pub key: Vec<u8>,
    }

    impl StorageKey {
        /// Create a storage key from its contract and raw key bytes.
        pub fn new(contract: UInt160, key: Vec<u8>) -> Self {
            Self { contract, key }
        }
    }

    /// Storage value used by the in-memory world-state test double.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct StorageItem {
        /// Raw storage value bytes.
        pub value: Vec<u8>,
    }

    impl StorageItem {
        /// Create a storage item from raw bytes.
        pub fn new(value: Vec<u8>) -> Self {
            Self { value }
        }

        /// Return the raw storage value bytes.
        pub fn as_bytes(&self) -> &[u8] {
            &self.value
        }
    }

    /// Account balances used by integration-test state fixtures.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AccountState {
        account: UInt160,
        neo_balance: u64,
        gas_balance: u64,
    }

    impl AccountState {
        /// Create an account state with explicit NEO and GAS balances.
        pub fn with_balances(account: UInt160, neo_balance: u64, gas_balance: u64) -> Self {
            Self {
                account,
                neo_balance,
                gas_balance,
            }
        }

        /// Return the NEO balance.
        pub fn neo_balance(&self) -> u64 {
            self.neo_balance
        }

        /// Return the GAS balance.
        pub fn gas_balance(&self) -> u64 {
            self.gas_balance
        }
    }

    /// Batched world-state changes for integration tests.
    #[derive(Debug, Clone, Default)]
    pub struct StateChanges {
        /// Storage changes keyed by storage key; `None` deletes the entry.
        pub storage: BTreeMap<StorageKey, Option<StorageItem>>,
        /// Account changes keyed by account hash; `None` deletes the account.
        pub accounts: BTreeMap<UInt160, Option<AccountState>>,
    }

    impl StateChanges {
        /// Create an empty change set.
        pub fn new() -> Self {
            Self::default()
        }
    }

    /// Minimal world-state interface used by integration tests.
    pub trait WorldState {
        /// Return the current block height represented by the state.
        fn height(&self) -> u32;
        /// Commit a batch of state changes.
        fn commit(&mut self, changes: StateChanges) -> Result<(), String>;
        /// Return a storage item by key.
        fn get_storage(&self, key: &StorageKey) -> Result<Option<StorageItem>, String>;
        /// Return an account state by account hash.
        fn get_account(&self, account: &UInt160) -> Result<Option<AccountState>, String>;
    }

    /// In-memory world-state implementation for integration tests.
    #[derive(Debug, Clone, Default)]
    pub struct MemoryWorldState {
        storage: BTreeMap<StorageKey, StorageItem>,
        accounts: BTreeMap<UInt160, AccountState>,
    }

    impl MemoryWorldState {
        /// Create an empty in-memory world state.
        pub fn new() -> Self {
            Self::default()
        }
    }

    impl WorldState for MemoryWorldState {
        fn height(&self) -> u32 {
            0
        }

        fn commit(&mut self, changes: StateChanges) -> Result<(), String> {
            for (key, value) in changes.storage {
                match value {
                    Some(item) => {
                        self.storage.insert(key, item);
                    }
                    None => {
                        self.storage.remove(&key);
                    }
                }
            }

            for (account, value) in changes.accounts {
                match value {
                    Some(state) => {
                        self.accounts.insert(account, state);
                    }
                    None => {
                        self.accounts.remove(&account);
                    }
                }
            }

            Ok(())
        }

        fn get_storage(&self, key: &StorageKey) -> Result<Option<StorageItem>, String> {
            Ok(self.storage.get(key).cloned())
        }

        fn get_account(&self, account: &UInt160) -> Result<Option<AccountState>, String> {
            Ok(self.accounts.get(account).cloned())
        }
    }

    /// Deterministic state-root test helper.
    #[derive(Debug, Clone)]
    pub struct StateTrieManager {
        full_state: bool,
        root_hash: Option<UInt256>,
        current_index: u32,
    }

    impl StateTrieManager {
        /// Create a state-root helper.
        pub fn new(full_state: bool) -> Self {
            Self {
                full_state,
                root_hash: None,
                current_index: 0,
            }
        }

        /// Return the current root hash, if any changes have been applied.
        pub fn root_hash(&self) -> Option<UInt256> {
            self.root_hash
        }

        /// Return the current block index represented by the helper.
        pub fn current_index(&self) -> u32 {
            self.current_index
        }

        /// Apply changes and compute a deterministic root hash for tests.
        pub fn apply_changes(
            &mut self,
            block_index: u32,
            changes: &StateChanges,
        ) -> Result<UInt256, String> {
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&block_index.to_le_bytes());
            bytes.push(u8::from(self.full_state));

            for (key, value) in &changes.storage {
                bytes.extend_from_slice(&key.contract.to_array());
                bytes.extend_from_slice(&(key.key.len() as u32).to_le_bytes());
                bytes.extend_from_slice(&key.key);
                match value {
                    Some(item) => {
                        bytes.push(1);
                        bytes.extend_from_slice(&(item.value.len() as u32).to_le_bytes());
                        bytes.extend_from_slice(&item.value);
                    }
                    None => bytes.push(0),
                }
            }

            for (account, value) in &changes.accounts {
                bytes.extend_from_slice(&account.to_array());
                match value {
                    Some(state) => {
                        bytes.push(1);
                        bytes.extend_from_slice(&state.account.to_array());
                        bytes.extend_from_slice(&state.neo_balance.to_le_bytes());
                        bytes.extend_from_slice(&state.gas_balance.to_le_bytes());
                    }
                    None => bytes.push(0),
                }
            }

            let root = UInt256::from(Crypto::hash256(&bytes));
            self.root_hash = Some(root);
            self.current_index = block_index;
            Ok(root)
        }

        /// Reset the helper to a known root and block index.
        pub fn reset_to_root(&mut self, root: UInt256, current_index: u32) {
            self.root_hash = Some(root);
            self.current_index = current_index;
        }
    }
}
