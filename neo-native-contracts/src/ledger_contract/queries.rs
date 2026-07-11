//! Ledger read-provider helpers.
//!
//! Keeps snapshot queries, trace-window checks, and index/hash resolution out
//! of the contract root while preserving the C# storage and wire formats.

use super::LedgerContract;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_io::{MemoryReader, Serializable};
use neo_payloads::TrimmedBlock;
use neo_primitives::{UInt160, UInt256};
use neo_storage::CacheRead;
use neo_storage::persistence::DataCache;

impl LedgerContract {
    /// Returns the current block index (height) of the blockchain.
    ///
    /// Reads the current-block pointer (prefix `12`) written by the
    /// block-persist pipeline. C# indexes the storage item directly and
    /// faults if the pointer is absent.
    pub fn current_index<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        self.optional_current_tip(snapshot)?
            .map(|(_, index)| index)
            .ok_or_else(|| CoreError::invalid_data("LedgerContract current block is missing"))
    }

    /// Returns the current block hash of the blockchain.
    ///
    /// Reads the current-block pointer (prefix `12`) written by the
    /// block-persist pipeline. C# indexes the storage item directly and
    /// faults if the pointer is absent.
    pub fn current_hash<B: CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<UInt256> {
        self.optional_current_tip(snapshot)?
            .map(|(hash, _)| hash)
            .ok_or_else(|| CoreError::invalid_data("LedgerContract current block is missing"))
    }

    /// Returns the current hash/index pair, or `None` for an uninitialized
    /// Ledger, from one coherent storage-item read.
    pub fn optional_current_tip<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<Option<(UInt256, u32)>> {
        let key = Self::current_block_storage_key();
        let Some(item) = snapshot.get(&key) else {
            return Ok(None);
        };
        let bytes = item.value_bytes().into_owned();
        Self::deserialize_hash_index_state(&bytes).map(Some)
    }

    /// Returns the per-transaction state for the given transaction
    /// hash, or `None` if no record exists under the key.
    ///
    /// The on-disk format (prefix `11` + 32-byte hash) is the C#
    /// `TransactionState` interoperable stack item serialized with
    /// `BinarySerializer` (TransactionState.cs `ToStackItem`):
    /// ```text
    /// Struct[Integer(BlockIndex)]                                  - conflict stub
    /// Struct[Integer(BlockIndex), ByteString(tx bytes), Integer((byte)State)]
    /// ```
    ///
    /// Like C#'s raw `item.GetInteroperable<TransactionState>()`, this
    /// surfaces conflict stubs as `Some` with `transaction == None`;
    /// the C# *public* `GetTransactionState` null-filter on stubs is
    /// applied by [`Self::contains_transaction`] and by the contract
    /// methods, which all check `transaction.is_some()`.
    pub fn get_transaction_state<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        tx_hash: &UInt256,
    ) -> CoreResult<Option<neo_payloads::TransactionState>> {
        let key = Self::transaction_storage_key(tx_hash);
        let Some(item) = snapshot.get(&key) else {
            return Ok(None);
        };
        let bytes = item.value_bytes().into_owned();
        Ok(Some(Self::decode_transaction_state(&bytes)?))
    }

    /// C# `LedgerContract.ContainsTransaction`: whether the ledger
    /// holds a **full** transaction record for the hash. A conflict
    /// stub (a `TransactionState` whose `Transaction` is null) does
    /// NOT count - C# `GetTransactionState` returns null for stubs.
    pub fn contains_transaction<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        tx_hash: &UInt256,
    ) -> CoreResult<bool> {
        Ok(self
            .get_transaction_state(snapshot, tx_hash)?
            .is_some_and(|state| state.transaction.is_some()))
    }

    /// C# `LedgerContract.ContainsConflictHash` (LedgerContract.cs:211):
    /// whether the chain contains a *traceable* conflict record for
    /// `hash` registered by an on-chain transaction sharing at least
    /// one of `signers`. The bare-hash stub is checked first (it must
    /// exist, be a stub - not a full transaction - and be traceable),
    /// then the per-signer stubs (`Prefix_Transaction + hash + signer`).
    pub fn contains_conflict_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        let current = self.current_index(snapshot)?;

        // C#: the dummy stub defines whether any conflict record exists.
        match self.get_transaction_state(snapshot, hash)? {
            Some(stub)
                if stub.transaction.is_none()
                    && Self::is_within_trace_window(
                        stub.block_index,
                        current,
                        max_traceable_blocks,
                    ) => {}
            _ => return Ok(false),
        }

        // At least one conflict record found: check signer intersection.
        for signer in signers {
            let key = Self::conflict_signer_storage_key(hash, signer);
            if let Some(item) = snapshot.get(&key) {
                let bytes = item.value_bytes().into_owned();
                let state = Self::decode_transaction_state(&bytes)?;
                if Self::is_within_trace_window(state.block_index, current, max_traceable_blocks) {
                    return Ok(true);
                }
            }
        }
        Ok(false)
    }

    /// Returns the block hash for the given block index, or `None` if
    /// the block has not been persisted yet.
    pub fn get_block_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        index: u32,
    ) -> CoreResult<Option<UInt256>> {
        let key = Self::block_hash_storage_key(index);
        match snapshot.get(&key) {
            Some(item) => {
                let bytes = item.value_bytes().into_owned();
                let hash = crate::args::bytes_to_hash256(&bytes, "invalid block hash bytes")?;
                Ok(Some(hash))
            }
            None => Ok(None),
        }
    }

    /// Returns the trimmed block (header + transaction hashes) stored under
    /// `Prefix_Block` + hash, or `None` if no block with that hash has been
    /// persisted (C# `LedgerContract.GetTrimmedBlock`). The on-disk payload is
    /// the `ISerializable` form written by `OnPersist`
    /// (`TrimmedBlock.Create(block).ToArray()`).
    pub fn get_trimmed_block<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        hash: &UInt256,
    ) -> CoreResult<Option<TrimmedBlock>> {
        let key = Self::block_storage_key(hash);
        match snapshot.get(&key) {
            Some(item) => {
                let bytes = item.value_bytes().into_owned();
                let mut reader = MemoryReader::new(&bytes);
                let block = TrimmedBlock::deserialize(&mut reader)
                    .map_err(|e| CoreError::serialization(e.to_string()))?;
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    /// Resolves the `byte[] indexOrHash` argument shared by C# `GetBlock` and
    /// `GetTransactionFromBlock` to a block hash:
    /// - fewer than 32 bytes -> a `BigInteger` block index, checked-cast to
    ///   `uint` (out-of-range faults, matching C# `(uint)`), then looked up via
    ///   the block-hash index (absent index -> `None`);
    /// - exactly 32 bytes -> the bytes are the `UInt256` hash;
    /// - any other length -> rejected (C# `ArgumentException`).
    pub(in crate::ledger_contract) fn resolve_block_hash<B: CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        index_or_hash: &[u8],
    ) -> CoreResult<Option<UInt256>> {
        match index_or_hash.len().cmp(&32) {
            std::cmp::Ordering::Less => {
                let index = crate::args::raw_integer_bytes_to_u32(
                    index_or_hash,
                    "LedgerContract: block index",
                )
                .map_err(|_| {
                    CoreError::invalid_operation("LedgerContract: block index out of uint range")
                })?;
                self.get_block_hash(snapshot, index)
            }
            std::cmp::Ordering::Equal => {
                let hash =
                    crate::args::bytes_to_hash256(index_or_hash, "LedgerContract: bad block hash")?;
                Ok(Some(hash))
            }
            std::cmp::Ordering::Greater => Err(CoreError::invalid_operation(format!(
                "Invalid indexOrHash length: {}",
                index_or_hash.len()
            ))),
        }
    }

    /// Mirrors C# `LedgerContract.IsTraceableBlock(engine, index)`: resolves the
    /// effective `MaxTraceableBlocks` (pre-`HF_Echidna`: the protocol setting;
    /// from `HF_Echidna`: the Policy storage value) and the current height, then
    /// applies the trace-window test.
    pub(in crate::ledger_contract) fn is_traceable_block<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &ApplicationEngine<P, D, B>,
        index: u32,
    ) -> CoreResult<bool> {
        let max_traceable_blocks = crate::PolicyContract::new().max_traceable_blocks(engine)?;
        let snapshot = engine.snapshot_cache();
        let current = LedgerContract::new().current_index(&snapshot)?;
        Ok(Self::is_within_trace_window(
            index,
            current,
            max_traceable_blocks,
        ))
    }

    /// Returns whether `index` remains inside the C# Ledger trace window.
    ///
    /// An index is traceable when it is not in the future and lies within the
    /// last `max_traceable_blocks` blocks. C# uses unchecked `uint` addition,
    /// so `wrapping_add` preserves its overflow corner. Hot and static-file
    /// ledger providers share this pure helper.
    #[must_use]
    pub fn is_within_trace_window(index: u32, current: u32, max_traceable_blocks: u32) -> bool {
        if index > current {
            return false;
        }
        index.wrapping_add(max_traceable_blocks) > current
    }
}
