//! # neo-test-fixtures
//!
//! Shared test fixtures for Neo workspace integration tests.
//!
//! Provides reusable helpers for constructing transactions, ledger blocks,
//! and seeding a `StoreCache` with block/transaction state — eliminating the
//! copy-pasted fixture code that was duplicated between `neo-rpc`'s
//! integration tests and internal test modules.
//!
//! ## Boundary
//!
//! This development-only crate constructs canonical test values through public
//! workspace APIs. Production crates must not depend on it.
//!
//! ## Contents
//!
//! - [`TestTransactionBuilder`]: fluent builder for Neo transactions with
//!   sensible defaults (nonce, script, signer, witness).
//! - [`test_chain_spec`]: deterministic complete chain specs for tests with
//!   custom protocol settings.
//! - [`try_make_ledger_block`]: constructs a `Block` at the given index, looking
//!   up the previous hash via `LedgerContract`.
//! - [`try_store_block`] / [`try_store_block_with_vmstate`]: writes block,
//!   transaction, and hash-index entries into a `StoreCache`, matching the
//!   on-disk format the `LedgerContract` reader expects.

use neo_error::{CoreError, CoreResult};
use neo_io::SerializableExtensions;
use neo_native_contracts::LedgerContract;
use neo_payloads::TrimmedBlock;
use neo_payloads::block::Block;
use neo_payloads::header::Header as BlockHeader;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::witness::Witness;
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_storage::persistence::{Store, StoreCache};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::VmState;
use std::sync::Arc;

/// Builds a deterministic private chain specification around test protocol
/// settings.
///
/// This is the single cross-crate owner for tests that need custom hardfork or
/// committee settings without reintroducing `ProtocolSettings` as a second
/// runtime root.
#[must_use]
pub fn test_chain_spec(settings: neo_config::ProtocolSettings) -> Arc<neo_config::NeoChainSpec> {
    let committee: Vec<String> = settings
        .standby_committee
        .iter()
        .map(|key| neo_primitives::hex_util::encode_hex(key.as_bytes()))
        .collect();
    let validator_count = usize::try_from(settings.validators_count)
        .expect("test validator count must be non-negative");
    let validators = committee
        .iter()
        .take(validator_count)
        .cloned()
        .map(|public_key| neo_config::GenesisValidator {
            public_key,
            name: None,
        })
        .collect();
    let genesis = neo_config::GenesisConfig {
        timestamp: neo_config::GENESIS_TIMESTAMP_MS,
        nonce: neo_config::GENESIS_NONCE,
        validators,
        committee,
        distribution: Vec::new(),
        contracts: Vec::new(),
    };

    Arc::new(
        neo_config::NeoChainSpec::private("workspace-test", settings, genesis, None)
            .expect("test settings must form a complete chain specification"),
    )
}

/// Storage-key prefixes used by the `LedgerContract` (C# `Prefix_*` constants).
mod prefix {
    pub const BLOCK: u8 = 0x05;
    pub const BLOCK_HASH: u8 = 0x09;
    pub const TRANSACTION: u8 = 0x0b;
    pub const CURRENT_BLOCK: u8 = 0x0c;
}

/// A fluent builder for constructing test [`Transaction`] values with
/// sensible defaults.
///
/// # Defaults
///
/// | Field | Default |
/// |-------|---------|
/// | `nonce` | `1` |
/// | `script` | `[0x51]` (`PUSH1`) |
/// | `signers` | single `CalledByEntry` signer at `UInt160::zero()` |
/// | `witnesses` | single empty witness |
/// | `network_fee` | `0` |
/// | `system_fee` | `0` |
/// | `valid_until_block` | `0` |
///
/// # Example
///
/// ```rust,ignore
/// use neo_test_fixtures::TestTransactionBuilder;
/// use neo_primitives::{UInt160, WitnessScope};
///
/// let tx = TestTransactionBuilder::new()
///     .nonce(42)
///     .network_fee(1_0000_0000)
///     .signer(UInt160::from([7u8; 20]), WitnessScope::GLOBAL)
///     .build();
/// ```
pub struct TestTransactionBuilder {
    nonce: u32,
    script: Vec<u8>,
    signers: Vec<Signer>,
    witnesses: Vec<Witness>,
    network_fee: i64,
    system_fee: i64,
    valid_until_block: u32,
}

impl Default for TestTransactionBuilder {
    fn default() -> Self {
        Self {
            nonce: 1,
            script: vec![0x51], // PUSH1
            signers: vec![Signer::new(UInt160::zero(), WitnessScope::CALLED_BY_ENTRY)],
            witnesses: vec![Witness::empty()],
            network_fee: 0,
            system_fee: 0,
            valid_until_block: 0,
        }
    }
}

impl TestTransactionBuilder {
    /// Creates a new builder with sensible defaults.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the transaction nonce.
    #[must_use]
    pub fn nonce(mut self, nonce: u32) -> Self {
        self.nonce = nonce;
        self
    }

    /// Sets the transaction script bytes.
    #[must_use]
    pub fn script(mut self, script: Vec<u8>) -> Self {
        self.script = script;
        self
    }

    /// Sets a single signer with the given account hash and witness scope.
    #[must_use]
    pub fn signer(mut self, account: UInt160, scope: WitnessScope) -> Self {
        self.signers = vec![Signer::new(account, scope)];
        self
    }

    /// Sets the network fee (in netFee units).
    #[must_use]
    pub fn network_fee(mut self, fee: i64) -> Self {
        self.network_fee = fee;
        self
    }

    /// Sets the system fee (in GAS fractions).
    #[must_use]
    pub fn system_fee(mut self, fee: i64) -> Self {
        self.system_fee = fee;
        self
    }

    /// Sets the `valid_until_block` field.
    #[must_use]
    pub fn valid_until_block(mut self, block: u32) -> Self {
        self.valid_until_block = block;
        self
    }

    /// Builds the final [`Transaction`].
    #[must_use]
    pub fn build(self) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_nonce(self.nonce);
        tx.set_script(self.script);
        tx.set_signers(self.signers);
        tx.set_witnesses(self.witnesses);
        tx.set_network_fee(self.network_fee);
        tx.set_system_fee(self.system_fee);
        tx.set_valid_until_block(self.valid_until_block);
        tx
    }
}

/// Constructs a [`Block`] at the given `index`, looking up the previous
/// block hash via [`LedgerContract`] on the supplied [`StoreCache`].
///
/// For `index == 0` the previous hash is `UInt256::zero()` (genesis).
/// The merkle root is computed from the transaction hashes, or
/// `UInt256::zero()` when the block is empty.
///
/// The block header uses deterministic test values: version `0`, timestamp `0`,
/// and `UInt160::zero()` as `next_consensus`, with a single empty witness.
pub fn try_make_ledger_block<S>(
    store: &StoreCache<S>,
    index: u32,
    transactions: Vec<Transaction>,
) -> CoreResult<Block>
where
    S: Store,
{
    let ledger = LedgerContract::new();
    let prev_hash = if index == 0 {
        UInt256::zero()
    } else {
        ledger
            .get_block_hash(store.data_cache(), index - 1)?
            .unwrap_or_else(UInt256::zero)
    };

    let merkle_root = if transactions.is_empty() {
        UInt256::zero()
    } else {
        let hashes: Vec<UInt256> = transactions.iter().map(|tx| tx.hash()).collect();
        neo_crypto::MerkleTree::compute_root(&hashes).unwrap_or_else(UInt256::zero)
    };

    let header = BlockHeader::new_with_witnesses(
        0,
        prev_hash,
        merkle_root,
        1,
        0,
        index,
        0,
        UInt160::zero(),
        vec![Witness::empty()],
    );

    Ok(Block::from_parts(header, transactions))
}

/// Writes a block and its transactions into a [`StoreCache`], matching the
/// on-disk format the [`LedgerContract`] reader expects.
///
/// Uses [`VmState::HALT`] for the persisted transaction state. Use
/// [`try_store_block_with_vmstate`] to override the VM state.
pub fn try_store_block<S>(store: &mut StoreCache<S>, block: &Block) -> CoreResult<()>
where
    S: Store,
{
    try_store_block_with_vmstate(store, block, VmState::HALT)
}

/// Writes a block and its transactions into a [`StoreCache`] with a custom
/// [`VmState`] for the persisted transaction state.
///
/// This writes:
/// - `Prefix_BlockHash` (big-endian index → block hash),
/// - `Prefix_Block` (hash → trimmed block),
/// - `Prefix_Transaction` (tx hash → `TransactionState` record),
/// - `Prefix_CurrentBlock` (hash + index pointer),
///
/// then commits the store and propagates any backend failure.
pub fn try_store_block_with_vmstate<S>(
    store: &mut StoreCache<S>,
    block: &Block,
    vmstate: VmState,
) -> CoreResult<()>
where
    S: Store,
{
    let hash = block.hash();
    let index = block.index();

    // Prefix_BlockHash: big-endian index → block hash.
    let mut hash_key_bytes = Vec::with_capacity(1 + 4);
    hash_key_bytes.push(prefix::BLOCK_HASH);
    hash_key_bytes.extend_from_slice(&index.to_be_bytes());
    let hash_key = StorageKey::new(LedgerContract::ID, hash_key_bytes);
    store.add(hash_key, StorageItem::from_bytes(hash.to_bytes().to_vec()));

    // Prefix_Block: hash → TrimmedBlock.
    let trimmed = TrimmedBlock::from_block(block)?;
    let trimmed_bytes = trimmed.to_array()?;
    let mut block_key_bytes = Vec::with_capacity(1 + 32);
    block_key_bytes.push(prefix::BLOCK);
    block_key_bytes.extend_from_slice(&hash.to_bytes());
    let block_key = StorageKey::new(LedgerContract::ID, block_key_bytes);
    store.add(block_key, StorageItem::from_bytes(trimmed_bytes));

    // Prefix_Transaction: tx hash → TransactionState record.
    for tx in &block.transactions {
        let record =
            LedgerContract::new().serialize_persisted_transaction_state(index, vmstate, tx)?;

        let mut tx_key_bytes = Vec::with_capacity(1 + 32);
        tx_key_bytes.push(prefix::TRANSACTION);
        tx_key_bytes.extend_from_slice(&tx.hash().to_bytes());
        let tx_key = StorageKey::new(LedgerContract::ID, tx_key_bytes);
        store.add(tx_key, StorageItem::from_bytes(record));
    }

    // Prefix_CurrentBlock: HashIndexState pointer.
    let current_bytes = LedgerContract::new().serialize_hash_index_state(&hash, index)?;
    let current_key = StorageKey::new(LedgerContract::ID, vec![prefix::CURRENT_BLOCK]);
    store.add(current_key, StorageItem::from_bytes(current_bytes));
    store
        .try_commit()
        .map_err(|error| CoreError::io(format!("commit fixture block: {error}")))?;
    Ok(())
}
