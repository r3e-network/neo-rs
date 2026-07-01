//! # neo-native-contracts::ledger_contract
//!
//! Native Ledger contract storage and query behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `wire`: Wire encoders, decoders, and deterministic network framing
//!   helpers.
//! - `tests`: Module-local tests and regression coverage.

use crate::hashes::LEDGER_CONTRACT_HASH;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_io::{MemoryReader, Serializable};
use neo_payloads::TrimmedBlock;
use neo_primitives::{UInt160, UInt256};
use neo_storage::persistence::DataCache;
use neo_vm_rs::VmState as VMState;
use num_bigint::BigInt;

mod metadata;
mod storage;
mod wire;

native_contract_handle!(
    /// Static accessor for the LedgerContract native contract.
    pub struct LedgerContract {
        id: -4,
        contract_name: "LedgerContract",
        hash: LEDGER_CONTRACT_HASH,
    }
);

impl LedgerContract {
    /// Returns the current block index (height) of the blockchain.
    ///
    /// Reads the current-block pointer (prefix `12`) written by the
    /// block-persist pipeline. C# indexes the storage item directly and
    /// faults if the pointer is absent.
    pub fn current_index(&self, snapshot: &DataCache) -> CoreResult<u32> {
        let key = Self::current_block_storage_key();
        let item = snapshot
            .get(&key)
            .ok_or_else(|| CoreError::invalid_data("LedgerContract current block is missing"))?;
        let bytes = item.value_bytes().into_owned();
        let (_, index) = Self::deserialize_hash_index_state(&bytes)?;
        Ok(index)
    }

    /// Returns the current block hash of the blockchain.
    ///
    /// Reads the current-block pointer (prefix `12`) written by the
    /// block-persist pipeline. C# indexes the storage item directly and
    /// faults if the pointer is absent.
    pub fn current_hash(&self, snapshot: &DataCache) -> CoreResult<UInt256> {
        let key = Self::current_block_storage_key();
        let item = snapshot
            .get(&key)
            .ok_or_else(|| CoreError::invalid_data("LedgerContract current block is missing"))?;
        let bytes = item.value_bytes().into_owned();
        let (hash, _) = Self::deserialize_hash_index_state(&bytes)?;
        Ok(hash)
    }

    /// Returns the per-transaction state for the given transaction
    /// hash, or `None` if no record exists under the key.
    ///
    /// The on-disk format (prefix `11` + 32-byte hash) is the C#
    /// `TransactionState` interoperable stack item serialized with
    /// `BinarySerializer` (TransactionState.cs `ToStackItem`):
    /// ```text
    /// Struct[Integer(BlockIndex)]                                  — conflict stub
    /// Struct[Integer(BlockIndex), ByteString(tx bytes), Integer((byte)State)]
    /// ```
    ///
    /// Like C#'s raw `item.GetInteroperable<TransactionState>()`, this
    /// surfaces conflict stubs as `Some` with `transaction == None`;
    /// the C# *public* `GetTransactionState` null-filter on stubs is
    /// applied by [`Self::contains_transaction`] and by the contract
    /// methods, which all check `transaction.is_some()`.
    pub fn get_transaction_state(
        &self,
        snapshot: &DataCache,
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
    /// NOT count — C# `GetTransactionState` returns null for stubs.
    pub fn contains_transaction(
        &self,
        snapshot: &DataCache,
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
    /// exist, be a stub — not a full transaction — and be traceable),
    /// then the per-signer stubs (`Prefix_Transaction + hash + signer`).
    pub fn contains_conflict_hash(
        &self,
        snapshot: &DataCache,
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
    pub fn get_block_hash(&self, snapshot: &DataCache, index: u32) -> CoreResult<Option<UInt256>> {
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
    pub fn get_trimmed_block(
        &self,
        snapshot: &DataCache,
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
    /// - fewer than 32 bytes → a `BigInteger` block index, checked-cast to `uint`
    ///   (out-of-range faults, matching C# `(uint)`), then looked up via the
    ///   block-hash index (absent index → `None`);
    /// - exactly 32 bytes → the bytes are the `UInt256` hash;
    /// - any other length → rejected (C# `ArgumentException`).
    fn resolve_block_hash(
        &self,
        snapshot: &DataCache,
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
    /// effective `MaxTraceableBlocks` (pre-`HF_Echidna`: the protocol setting; from
    /// `HF_Echidna`: the Policy storage value) and the current height, then applies
    /// the trace-window test.
    fn is_traceable_block(&self, engine: &ApplicationEngine, index: u32) -> CoreResult<bool> {
        let max_traceable_blocks = crate::PolicyContract::new().max_traceable_blocks(engine)?;
        let snapshot = engine.snapshot_cache();
        let current = LedgerContract::new().current_index(&snapshot)?;
        Ok(Self::is_within_trace_window(
            index,
            current,
            max_traceable_blocks,
        ))
    }

    /// Pure core of C# `LedgerContract.IsTraceableBlock(snapshot, index, mtb)`:
    /// a block `index` is traceable at height `current` iff it is not in the future
    /// and lies within the last `max_traceable_blocks` blocks. C# uses unchecked
    /// `uint` addition, so `wrapping_add` is used to match the (unreachable) overflow
    /// corner byte-for-byte.
    fn is_within_trace_window(index: u32, current: u32, max_traceable_blocks: u32) -> bool {
        if index > current {
            return false;
        }
        index.wrapping_add(max_traceable_blocks) > current
    }
}

impl NativeContract for LedgerContract {
    native_contract_identity!(LedgerContract);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::LEDGER_CONTRACT_METHODS
    }

    fn transaction_state(
        &self,
        snapshot: &neo_storage::DataCache,
        tx_hash: &UInt256,
    ) -> CoreResult<Option<neo_payloads::TransactionState>> {
        self.get_transaction_state(snapshot, tx_hash)
    }

    fn trimmed_block(
        &self,
        snapshot: &neo_storage::DataCache,
        block_hash: &UInt256,
    ) -> CoreResult<Option<TrimmedBlock>> {
        self.get_trimmed_block(snapshot, block_hash)
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // All wired methods are read-only queries over persisted ledger state,
        // served from the engine's snapshot (C# `RequiredCallFlags = ReadStates`).
        let snapshot = engine.snapshot_cache();
        match method {
            "currentIndex" => Ok(BigInt::from(self.current_index(&snapshot)?).to_signed_bytes_le()),
            "currentHash" => Ok(self.current_hash(&snapshot)?.to_bytes()),
            "getTransactionHeight" => {
                let hash =
                    crate::args::raw_hash256(args, 0, "LedgerContract::getTransactionHeight")?;
                // C# `GetTransactionState` returns null for a conflict stub (its
                // `Transaction` is null), and `getTransactionHeight` returns -1 for
                // an absent or untraceable transaction; otherwise `(int)BlockIndex`.
                let height = match self.get_transaction_state(&snapshot, &hash)? {
                    Some(state)
                        if state.transaction.is_some()
                            && self.is_traceable_block(engine, state.block_index)? =>
                    {
                        i64::from(state.block_index as i32)
                    }
                    _ => -1,
                };
                Ok(BigInt::from(height).to_signed_bytes_le())
            }
            "getTransactionVMState" => {
                let hash =
                    crate::args::raw_hash256(args, 0, "LedgerContract::getTransactionVMState")?;
                // C# returns VMState.NONE for an absent, conflict-stub, or
                // untraceable transaction; otherwise the recorded execution state.
                let vm_state = match self.get_transaction_state(&snapshot, &hash)? {
                    Some(state) if state.transaction.is_some() => {
                        if self.is_traceable_block(engine, state.block_index)? {
                            state.state.to_byte()
                        } else {
                            VMState::NONE.to_byte()
                        }
                    }
                    _ => VMState::NONE.to_byte(),
                };
                Ok(BigInt::from(vm_state).to_signed_bytes_le())
            }
            "getTransaction" => {
                let hash = crate::args::raw_hash256(args, 0, "LedgerContract::getTransaction")?;
                // C# returns the transaction (Array via ToStackItem) for a
                // traceable full record; null (empty payload) for an absent,
                // conflict-stub, or untraceable transaction.
                match self.get_transaction_state(&snapshot, &hash)? {
                    Some(state) => {
                        if let Some(tx) = &state.transaction {
                            if self.is_traceable_block(engine, state.block_index)? {
                                Self::transaction_to_bytes(tx, "getTransaction")
                            } else {
                                Ok(Vec::new())
                            }
                        } else {
                            Ok(Vec::new())
                        }
                    }
                    None => Ok(Vec::new()),
                }
            }
            "getTransactionSigners" => {
                let hash =
                    crate::args::raw_hash256(args, 0, "LedgerContract::getTransactionSigners")?;
                // C# returns the transaction's Signer[] (Array via ToStackItem) for
                // a traceable full record; null (empty payload) otherwise.
                match self.get_transaction_state(&snapshot, &hash)? {
                    Some(state) => {
                        if let Some(tx) = &state.transaction {
                            if self.is_traceable_block(engine, state.block_index)? {
                                Self::signers_to_bytes(tx.signers(), "getTransactionSigners")
                            } else {
                                Ok(Vec::new())
                            }
                        } else {
                            Ok(Vec::new())
                        }
                    }
                    None => Ok(Vec::new()),
                }
            }
            "getBlock" => {
                let index_or_hash = crate::args::raw_arg(args, 0, "LedgerContract::getBlock")
                    .map_err(|_| {
                        CoreError::invalid_operation(
                            "LedgerContract::getBlock requires an indexOrHash",
                        )
                    })?;
                // C#: resolve the index/hash to a block hash, load the trimmed
                // block, and return it (Array via ToStackItem) only if traceable;
                // null (empty payload) for an absent or untraceable block.
                let Some(hash) = self.resolve_block_hash(&snapshot, index_or_hash)? else {
                    return Ok(Vec::new());
                };
                match self.get_trimmed_block(&snapshot, &hash)? {
                    Some(block) if self.is_traceable_block(engine, block.index())? => {
                        Self::trimmed_block_to_bytes(&block, "getBlock")
                    }
                    _ => Ok(Vec::new()),
                }
            }
            "getTransactionFromBlock" => {
                let index_or_hash = crate::args::raw_arg(
                    args,
                    0,
                    "LedgerContract::getTransactionFromBlock",
                )
                .map_err(|_| {
                    CoreError::invalid_operation(
                        "LedgerContract::getTransactionFromBlock requires a blockIndexOrHash",
                    )
                })?;
                let tx_index_bytes =
                    crate::args::raw_arg(args, 1, "LedgerContract::getTransactionFromBlock")
                        .map_err(|_| {
                            CoreError::invalid_operation(
                                "LedgerContract::getTransactionFromBlock requires a txIndex",
                            )
                        })?;
                let tx_index = crate::args::raw_integer_bytes_to_i32(
                    tx_index_bytes,
                    "LedgerContract::getTransactionFromBlock: txIndex",
                )
                .map_err(|_| {
                    CoreError::invalid_operation(
                        "LedgerContract::getTransactionFromBlock: txIndex out of int range",
                    )
                })?;
                let Some(hash) = self.resolve_block_hash(&snapshot, index_or_hash)? else {
                    return Ok(Vec::new());
                };
                // The block must exist and be traceable; otherwise null.
                let block = match self.get_trimmed_block(&snapshot, &hash)? {
                    Some(block) if self.is_traceable_block(engine, block.index())? => block,
                    _ => return Ok(Vec::new()),
                };
                // C# throws ArgumentOutOfRangeException for an out-of-range txIndex.
                if tx_index < 0 || tx_index as usize >= block.hashes.len() {
                    return Err(CoreError::invalid_operation(format!(
                        "LedgerContract::getTransactionFromBlock: txIndex {tx_index} out of range (len {})",
                        block.hashes.len()
                    )));
                }
                let tx_hash = block.hashes[tx_index as usize];
                // C# public GetTransaction(snapshot, hash): the transaction (no
                // extra traceability re-check, the block is already traceable),
                // or null for a conflict-stub/absent transaction.
                let tx = self
                    .get_transaction_state(&snapshot, &tx_hash)?
                    .and_then(|state| state.transaction);
                match tx {
                    Some(tx) => Self::transaction_to_bytes(&tx, "getTransactionFromBlock"),
                    None => Ok(Vec::new()),
                }
            }
            other => Err(CoreError::invalid_operation(format!(
                "LedgerContract method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
#[path = "../tests/ledger_contract/mod.rs"]
mod tests;
