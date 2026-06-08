//! Shared support types for workspace integration tests.

pub mod mempool {
    /// Lightweight fee-policy placeholder used by integration tests.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct FeePolicy;

    /// Test mempool configuration facade.
    #[derive(Debug, Clone)]
    pub struct MempoolConfig {
        pub max_transactions: usize,
        pub max_per_sender: usize,
        pub fee_policy: FeePolicy,
        pub enable_replacement: bool,
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
        pub fn new() -> Self {
            Self::with_config(MempoolConfig::default())
        }

        pub fn with_config(config: MempoolConfig) -> Self {
            Self { _config: config }
        }

        pub fn len(&self) -> usize {
            0
        }

        pub fn is_empty(&self) -> bool {
            true
        }

        pub fn get_top(&self, _count: usize) -> Vec<()> {
            Vec::new()
        }
    }
}

pub mod state {
    use neo_crypto::Crypto;
    use neo_primitives::{UInt160, UInt256};
    use std::collections::BTreeMap;

    #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct StorageKey {
        pub contract: UInt160,
        pub key: Vec<u8>,
    }

    impl StorageKey {
        pub fn new(contract: UInt160, key: Vec<u8>) -> Self {
            Self { contract, key }
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct StorageItem {
        pub value: Vec<u8>,
    }

    impl StorageItem {
        pub fn new(value: Vec<u8>) -> Self {
            Self { value }
        }

        pub fn as_bytes(&self) -> &[u8] {
            &self.value
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct AccountState {
        account: UInt160,
        neo_balance: u64,
        gas_balance: u64,
    }

    impl AccountState {
        pub fn with_balances(account: UInt160, neo_balance: u64, gas_balance: u64) -> Self {
            Self {
                account,
                neo_balance,
                gas_balance,
            }
        }

        pub fn neo_balance(&self) -> u64 {
            self.neo_balance
        }

        pub fn gas_balance(&self) -> u64 {
            self.gas_balance
        }
    }

    #[derive(Debug, Clone, Default)]
    pub struct StateChanges {
        pub storage: BTreeMap<StorageKey, Option<StorageItem>>,
        pub accounts: BTreeMap<UInt160, Option<AccountState>>,
    }

    impl StateChanges {
        pub fn new() -> Self {
            Self::default()
        }
    }

    pub trait WorldState {
        fn height(&self) -> u32;
        fn commit(&mut self, changes: StateChanges) -> Result<(), String>;
        fn get_storage(&self, key: &StorageKey) -> Result<Option<StorageItem>, String>;
        fn get_account(&self, account: &UInt160) -> Result<Option<AccountState>, String>;
    }

    #[derive(Debug, Clone, Default)]
    pub struct MemoryWorldState {
        storage: BTreeMap<StorageKey, StorageItem>,
        accounts: BTreeMap<UInt160, AccountState>,
    }

    impl MemoryWorldState {
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

    #[derive(Debug, Clone)]
    pub struct StateTrieManager {
        full_state: bool,
        root_hash: Option<UInt256>,
        current_index: u32,
    }

    impl StateTrieManager {
        pub fn new(full_state: bool) -> Self {
            Self {
                full_state,
                root_hash: None,
                current_index: 0,
            }
        }

        pub fn root_hash(&self) -> Option<UInt256> {
            self.root_hash
        }

        pub fn current_index(&self) -> u32 {
            self.current_index
        }

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

        pub fn reset_to_root(&mut self, root: UInt256, current_index: u32) {
            self.root_hash = Some(root);
            self.current_index = current_index;
        }
    }
}
