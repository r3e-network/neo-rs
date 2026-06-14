//! LedgerContract native contract.
//!
//! Concrete (non-stub) implementation of the LedgerContract's storage
//! query surface. Mirrors the canonical C# `LedgerContract` storage
//! layout so plugins, services, and the application engine can read
//! transaction state, block-hash-by-index, and the current block
//! pointer that other components (blockchain, consensus) write into
//! the snapshot.
//!
//! The full read/write surface (block storage, block-hash index, the
//! various persistent transaction records and conflict stubs) is
//! handled by the `neo-payloadschain` reth-style service; this crate only
//! provides the read-only query surface used by oracle service, RPC,
//! and the application engine.

use crate::hashes::LEDGER_CONTRACT_HASH;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, Interoperable, NativeContract, NativeMethod};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_payloads::{Transaction, TrimmedBlock};
use neo_primitives::{CallFlags, ContractParameterType, UInt160, UInt256};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::{ExecutionEngineLimits, VmState as VMState};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::any::Any;
use std::sync::LazyLock;

/// Storage prefix for the per-block-index → block-hash index.
const PREFIX_BLOCK_HASH: u8 = 9;
/// Storage prefix for the trimmed-block payload.
const PREFIX_BLOCK: u8 = 5;
/// Storage prefix for the per-transaction state record.
const PREFIX_TRANSACTION: u8 = 11;
/// Storage prefix for the current-block (hash, index) pointer.
const PREFIX_CURRENT_BLOCK: u8 = 12;

/// Static accessor for the LedgerContract native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct LedgerContract;

impl LedgerContract {
    /// Stable native contract id (matches C# `LedgerContract.Id`).
    pub const ID: i32 = -4;
    /// Stable native contract name (matches C# `LedgerContract.Name`).
    pub const NAME: &'static str = "LedgerContract";

    /// Constructs a new `LedgerContract` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the LedgerContract.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the script hash of the LedgerContract (static).
    pub fn script_hash() -> UInt160 {
        *LEDGER_CONTRACT_HASH
    }

    /// Returns the current block index (height) of the blockchain.
    ///
    /// Reads the current-block pointer (prefix `12`) written by the
    /// block-persist pipeline. Returns `0` when the pointer is
    /// missing (e.g. at genesis).
    pub fn current_index(&self, snapshot: &DataCache) -> CoreResult<u32> {
        let key = Self::current_block_storage_key(Self::ID);
        match snapshot.get(&key) {
            Some(item) => {
                let bytes = item.value_bytes().into_owned();
                let (_, index) = Self::deserialize_hash_index_state(&bytes)?;
                Ok(index)
            }
            None => Ok(0),
        }
    }

    /// Returns the current block hash of the blockchain.
    ///
    /// Reads the current-block pointer (prefix `12`) written by the
    /// block-persist pipeline. Returns the zero hash when the pointer
    /// is missing.
    pub fn current_hash(&self, snapshot: &DataCache) -> CoreResult<UInt256> {
        let key = Self::current_block_storage_key(Self::ID);
        match snapshot.get(&key) {
            Some(item) => {
                let bytes = item.value_bytes().into_owned();
                let (hash, _) = Self::deserialize_hash_index_state(&bytes)?;
                Ok(hash)
            }
            None => Ok(UInt256::default()),
        }
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
        let key = Self::transaction_storage_key(Self::ID, tx_hash);
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
            let key = Self::conflict_signer_storage_key(Self::ID, hash, signer);
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
        let key = Self::block_hash_storage_key(Self::ID, index);
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
        let key = Self::block_storage_key(Self::ID, hash);
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
                let index = BigInt::from_signed_bytes_le(index_or_hash)
                    .to_u32()
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "LedgerContract: block index out of uint range",
                        )
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

    /// Serialises a `(hash, index)` pair into the C# `HashIndexState`
    /// wire format used for the current-block pointer: the interoperable
    /// stack item `Struct[ByteString(hash), Integer(index)]`
    /// (HashIndexState.cs `ToStackItem`) serialized with `BinarySerializer`
    /// — exactly what C# `StorageItem.GetInteroperable<HashIndexState>()`
    /// round-trips.
    pub fn serialize_hash_index_state(&self, hash: &UInt256, index: u32) -> CoreResult<Vec<u8>> {
        let item = StackItem::from_struct(vec![
            StackItem::from_byte_string(hash.to_bytes()),
            StackItem::from_int(BigInt::from(index)),
        ]);
        BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
            .map_err(|e| CoreError::serialization(format!("HashIndexState: {e}")))
    }

    /// Serialises a persisted transaction state into the C# wire format:
    /// the interoperable `Struct[Integer(BlockIndex), ByteString(tx bytes),
    /// Integer((byte)State)]` (TransactionState.cs `ToStackItem`)
    /// serialized with `BinarySerializer`.
    pub fn serialize_persisted_transaction_state(
        &self,
        block_index: u32,
        vm_state: VMState,
        tx: &Transaction,
    ) -> CoreResult<Vec<u8>> {
        let mut tx_writer = BinaryWriter::new();
        tx.serialize(&mut tx_writer)
            .map_err(|e| CoreError::serialization(e.to_string()))?;
        let item = StackItem::from_struct(vec![
            StackItem::from_int(BigInt::from(block_index)),
            StackItem::from_byte_string(tx_writer.into_bytes()),
            StackItem::from_int(BigInt::from(vm_state.to_byte())),
        ]);
        BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
            .map_err(|e| CoreError::serialization(format!("TransactionState: {e}")))
    }

    /// Serialises a conflict-stub record into the C# wire format: the
    /// interoperable `Struct[Integer(BlockIndex)]` of a `TransactionState`
    /// whose `Transaction` is null (TransactionState.cs `ToStackItem`).
    pub fn serialize_conflict_stub(&self, block_index: u32) -> CoreResult<Vec<u8>> {
        let item = StackItem::from_struct(vec![StackItem::from_int(BigInt::from(block_index))]);
        BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
            .map_err(|e| CoreError::serialization(format!("TransactionState stub: {e}")))
    }

    // ============================================================================
    // Storage-key helpers
    // ============================================================================

    #[inline]
    fn current_block_storage_key(contract_id: i32) -> StorageKey {
        StorageKey::new(contract_id, vec![PREFIX_CURRENT_BLOCK])
    }

    /// C# `CreateStorageKey(Prefix_BlockHash, uint bigEndianKey)`
    /// (NativeContract.cs:403 → `KeyBuilder.AddBigEndian(uint)`): the
    /// block index is encoded **big-endian** so the index keys sort in
    /// block order.
    #[inline]
    fn block_hash_storage_key(contract_id: i32, index: u32) -> StorageKey {
        StorageKey::new(
            contract_id,
            crate::keys::prefixed_with_u32_be(PREFIX_BLOCK_HASH, index),
        )
    }

    #[inline]
    fn transaction_storage_key(contract_id: i32, hash: &UInt256) -> StorageKey {
        StorageKey::new(
            contract_id,
            crate::keys::prefixed_with_hash256(PREFIX_TRANSACTION, hash),
        )
    }

    /// C# `CreateStorageKey(Prefix_Transaction, UInt256 hash, UInt160 signer)`
    /// — the per-signer conflict-stub key.
    #[inline]
    fn conflict_signer_storage_key(
        contract_id: i32,
        hash: &UInt256,
        signer: &UInt160,
    ) -> StorageKey {
        let mut key = Vec::with_capacity(1 + 32 + 20);
        key.push(PREFIX_TRANSACTION);
        key.extend_from_slice(&hash.to_bytes());
        key.extend_from_slice(&signer.to_bytes());
        StorageKey::new(contract_id, key)
    }

    #[inline]
    fn block_storage_key(contract_id: i32, hash: &UInt256) -> StorageKey {
        StorageKey::new(
            contract_id,
            crate::keys::prefixed_with_hash256(PREFIX_BLOCK, hash),
        )
    }

    // ============================================================================
    // Wire-format helpers
    // ============================================================================

    fn deserialize_hash_index_state(bytes: &[u8]) -> CoreResult<(UInt256, u32)> {
        let item = BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None)
            .map_err(|e| CoreError::invalid_data(format!("HashIndexState: {e}")))?;
        let StackItem::Struct(fields) = item else {
            return Err(CoreError::invalid_data(
                "HashIndexState record is not a Struct stack item",
            ));
        };
        let items = fields.items();
        if items.len() < 2 {
            return Err(CoreError::invalid_data(
                "HashIndexState struct is shorter than expected",
            ));
        }
        let hash_bytes = items[0]
            .as_bytes()
            .map_err(|e| CoreError::invalid_data(format!("HashIndexState hash: {e}")))?;
        let hash = crate::args::bytes_to_hash256(&hash_bytes, "invalid HashIndexState hash")?;
        let index = items[1]
            .as_int()
            .map_err(|e| CoreError::invalid_data(format!("HashIndexState index: {e}")))?
            .to_u32()
            .ok_or_else(|| CoreError::invalid_data("HashIndexState index out of uint range"))?;
        Ok((hash, index))
    }

    /// Decodes a `Prefix_Transaction` record: the C# `TransactionState`
    /// interoperable stack item (TransactionState.cs `FromStackItem`).
    /// `Struct[Integer]` is a conflict stub (no transaction);
    /// `Struct[Integer, ByteString, Integer]` is a full record.
    fn decode_transaction_state(bytes: &[u8]) -> CoreResult<neo_payloads::TransactionState> {
        let item = BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None)
            .map_err(|e| CoreError::invalid_data(format!("TransactionState: {e}")))?;
        let StackItem::Struct(fields) = item else {
            return Err(CoreError::invalid_data(
                "TransactionState record is not a Struct stack item",
            ));
        };
        let items = fields.items();
        let block_index = items
            .first()
            .ok_or_else(|| CoreError::invalid_data("TransactionState struct is empty"))?
            .as_int()
            .map_err(|e| CoreError::invalid_data(format!("TransactionState block index: {e}")))?
            .to_u32()
            .ok_or_else(|| CoreError::invalid_data("TransactionState block index out of range"))?;

        // C#: `if (@struct.Count == 1) return;` — conflict record.
        if items.len() == 1 {
            return Ok(neo_payloads::TransactionState::new(
                block_index,
                None,
                VMState::NONE,
            ));
        }
        if items.len() < 3 {
            return Err(CoreError::invalid_data(
                "TransactionState struct has an invalid field count",
            ));
        }

        let tx_bytes = items[1]
            .as_bytes()
            .map_err(|e| CoreError::invalid_data(format!("TransactionState tx bytes: {e}")))?;
        let mut tx_reader = MemoryReader::new(&tx_bytes);
        let tx = Transaction::deserialize(&mut tx_reader)
            .map_err(|e| CoreError::serialization(e.to_string()))?;
        let state_byte = items[2]
            .as_int()
            .map_err(|e| CoreError::invalid_data(format!("TransactionState vm state: {e}")))?
            .to_u8()
            .ok_or_else(|| {
                CoreError::invalid_data("TransactionState vm state out of byte range")
            })?;
        Ok(neo_payloads::TransactionState::new(
            block_index,
            Some(tx),
            VMState::from_byte(state_byte),
        ))
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

static LEDGER_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethod::new(
            "currentHash".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Hash256,
        ),
        NativeMethod::new(
            "currentIndex".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        NativeMethod::new(
            "getTransactionHeight".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash256],
            ContractParameterType::Integer,
        )
        .with_parameter_names(["hash"]),
        NativeMethod::new(
            "getTransactionVMState".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash256],
            ContractParameterType::Integer,
        )
        .with_parameter_names(["hash"]),
        NativeMethod::new(
            "getTransaction".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash256],
            ContractParameterType::Array,
        )
        .with_parameter_names(["hash"]),
        NativeMethod::new(
            "getTransactionSigners".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash256],
            ContractParameterType::Array,
        )
        .with_parameter_names(["hash"]),
        // getBlock(indexOrHash: ByteArray) -> Array (TrimmedBlock) | Null.
        NativeMethod::new(
            "getBlock".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::ByteArray],
            ContractParameterType::Array,
        )
        .with_parameter_names(["indexOrHash"]),
        // getTransactionFromBlock(blockIndexOrHash: ByteArray, txIndex: Integer)
        // -> Array (Transaction) | Null. C# CpuFee is 1 << 16 (heavier than the
        // other ledger reads because it loads a whole trimmed block).
        NativeMethod::new(
            "getTransactionFromBlock".to_string(),
            1 << 16,
            true,
            read_states,
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Array,
        )
        .with_parameter_names(["blockIndexOrHash", "txIndex"]),
    ]
});

impl NativeContract for LedgerContract {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &LEDGER_METHODS
    }

    fn as_any(&self) -> &dyn Any {
        self
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
                    Some(state) if state.transaction.is_some() => {
                        if self.is_traceable_block(engine, state.block_index)? {
                            i64::from(state.block_index as i32)
                        } else {
                            -1
                        }
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
                                let item = tx.to_stack_item().map_err(|e| {
                                    CoreError::invalid_operation(format!(
                                        "LedgerContract::getTransaction: stack item: {e}"
                                    ))
                                })?;
                                BinarySerializer::serialize(
                                    &item,
                                    &ExecutionEngineLimits::default(),
                                )
                                .map_err(|e| {
                                    CoreError::invalid_operation(format!(
                                        "LedgerContract::getTransaction: serialize: {e}"
                                    ))
                                })
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
                                let mut items = Vec::with_capacity(tx.signers().len());
                                for signer in tx.signers() {
                                    items.push(signer.to_stack_item().map_err(|e| {
                                        CoreError::invalid_operation(format!(
                                            "LedgerContract::getTransactionSigners: stack item: {e}"
                                        ))
                                    })?);
                                }
                                BinarySerializer::serialize(
                                    &StackItem::from_array(items),
                                    &ExecutionEngineLimits::default(),
                                )
                                .map_err(|e| {
                                    CoreError::invalid_operation(format!(
                                        "LedgerContract::getTransactionSigners: serialize: {e}"
                                    ))
                                })
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
                let index_or_hash = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("LedgerContract::getBlock requires an indexOrHash")
                })?;
                // C#: resolve the index/hash to a block hash, load the trimmed
                // block, and return it (Array via ToStackItem) only if traceable;
                // null (empty payload) for an absent or untraceable block.
                let Some(hash) = self.resolve_block_hash(&snapshot, index_or_hash)? else {
                    return Ok(Vec::new());
                };
                match self.get_trimmed_block(&snapshot, &hash)? {
                    Some(block) if self.is_traceable_block(engine, block.index())? => {
                        let item = block.to_stack_item().map_err(|e| {
                            CoreError::invalid_operation(format!(
                                "LedgerContract::getBlock: stack item: {e}"
                            ))
                        })?;
                        BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
                            .map_err(|e| {
                                CoreError::invalid_operation(format!(
                                    "LedgerContract::getBlock: serialize: {e}"
                                ))
                            })
                    }
                    _ => Ok(Vec::new()),
                }
            }
            "getTransactionFromBlock" => {
                let index_or_hash = args.first().ok_or_else(|| {
                    CoreError::invalid_operation(
                        "LedgerContract::getTransactionFromBlock requires a blockIndexOrHash",
                    )
                })?;
                let tx_index_bytes = args.get(1).ok_or_else(|| {
                    CoreError::invalid_operation(
                        "LedgerContract::getTransactionFromBlock requires a txIndex",
                    )
                })?;
                let tx_index = BigInt::from_signed_bytes_le(tx_index_bytes)
                    .to_i32()
                    .ok_or_else(|| {
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
                    Some(tx) => {
                        let item = tx.to_stack_item().map_err(|e| {
                            CoreError::invalid_operation(format!(
                                "LedgerContract::getTransactionFromBlock: stack item: {e}"
                            ))
                        })?;
                        BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
                            .map_err(|e| {
                                CoreError::invalid_operation(format!(
                                    "LedgerContract::getTransactionFromBlock: serialize: {e}"
                                ))
                            })
                    }
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
mod tests {
    use super::*;

    #[test]
    fn native_contract_surface() {
        let c = LedgerContract::new();
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "currentHash",
                "currentIndex",
                "getTransactionHeight",
                "getTransactionVMState",
                "getTransaction",
                "getTransactionSigners",
                "getBlock",
                "getTransactionFromBlock"
            ]
        );
        assert!(
            c.methods()
                .iter()
                .all(|m| m.safe && m.required_call_flags == CallFlags::READ_STATES.bits())
        );
        for name in ["getTransactionHeight", "getTransactionVMState"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert_eq!(m.parameters, vec![ContractParameterType::Hash256]);
            assert_eq!(m.return_type, ContractParameterType::Integer);
            assert_eq!(m.cpu_fee, 1 << 15);
        }
        for name in ["getTransaction", "getTransactionSigners"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert_eq!(m.parameters, vec![ContractParameterType::Hash256]);
            assert_eq!(m.return_type, ContractParameterType::Array);
            assert_eq!(m.cpu_fee, 1 << 15);
        }
        // getBlock takes a single ByteArray (indexOrHash) and returns an Array.
        let get_block = c.methods().iter().find(|m| m.name == "getBlock").unwrap();
        assert_eq!(get_block.parameters, vec![ContractParameterType::ByteArray]);
        assert_eq!(get_block.return_type, ContractParameterType::Array);
        assert_eq!(get_block.cpu_fee, 1 << 15);
        // getTransactionFromBlock takes (ByteArray, Integer) and is the only
        // ledger read with the heavier 1 << 16 CPU fee.
        let from_block = c
            .methods()
            .iter()
            .find(|m| m.name == "getTransactionFromBlock")
            .unwrap();
        assert_eq!(
            from_block.parameters,
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::Integer
            ]
        );
        assert_eq!(from_block.return_type, ContractParameterType::Array);
        assert_eq!(from_block.cpu_fee, 1 << 16);
    }

    #[test]
    fn get_trimmed_block_round_trips_through_storage() {
        use neo_io::BinaryWriter;
        use neo_payloads::{Header, TrimmedBlock};

        let cache = DataCache::new(false);
        let ledger = LedgerContract::new();

        let mut header = Header::new();
        header.set_index(77);
        header.set_nonce(u64::MAX);
        let block_hash = header.hash();

        let trimmed = TrimmedBlock::new(
            header,
            vec![
                UInt256::from_bytes(&[0x11u8; 32]).unwrap(),
                UInt256::from_bytes(&[0x22u8; 32]).unwrap(),
            ],
        );

        // Absent block -> None.
        assert!(
            ledger
                .get_trimmed_block(&cache, &block_hash)
                .unwrap()
                .is_none()
        );

        // Persist the trimmed block exactly as OnPersist does
        // (TrimmedBlock.ToArray() = ISerializable bytes) and read it back.
        let mut writer = BinaryWriter::new();
        trimmed.serialize(&mut writer).unwrap();
        cache.add(
            LedgerContract::block_storage_key(LedgerContract::ID, &block_hash),
            StorageItem::from_bytes(writer.into_bytes()),
        );

        let loaded = ledger
            .get_trimmed_block(&cache, &block_hash)
            .unwrap()
            .unwrap();
        assert_eq!(loaded.header.index(), 77);
        assert_eq!(loaded.header.nonce(), u64::MAX);
        assert_eq!(loaded.hashes, trimmed.hashes);
    }

    #[test]
    fn resolve_block_hash_handles_index_hash_and_bad_length() {
        let cache = DataCache::new(false);
        let ledger = LedgerContract::new();

        // Exactly 32 bytes: the argument is the hash itself.
        let raw = [0x5Au8; 32];
        assert_eq!(
            ledger.resolve_block_hash(&cache, &raw).unwrap(),
            Some(UInt256::from_bytes(&raw).unwrap())
        );

        // Fewer than 32 bytes: a block index resolved via the block-hash index.
        // Absent index -> None.
        assert_eq!(ledger.resolve_block_hash(&cache, &[5u8]).unwrap(), None);
        let indexed_hash = UInt256::from_bytes(&[0x7u8; 32]).unwrap();
        cache.add(
            LedgerContract::block_hash_storage_key(LedgerContract::ID, 5),
            StorageItem::from_bytes(indexed_hash.to_bytes()),
        );
        assert_eq!(
            ledger.resolve_block_hash(&cache, &[5u8]).unwrap(),
            Some(indexed_hash)
        );

        // More than 32 bytes: rejected (C# ArgumentException).
        assert!(ledger.resolve_block_hash(&cache, &[0u8; 33]).is_err());
    }

    #[test]
    fn trace_window_matches_csharp_is_traceable_block() {
        // current=100, mtb=10 => traceable indices are (90, 100].
        // Future block: never traceable.
        assert!(!LedgerContract::is_within_trace_window(101, 100, 10));
        // Lower boundary is exclusive: index + mtb must be strictly > current.
        // index=90 -> 90+10=100, not > 100 -> not traceable.
        assert!(!LedgerContract::is_within_trace_window(90, 100, 10));
        // index=91 -> 101 > 100 -> traceable; current index is traceable.
        assert!(LedgerContract::is_within_trace_window(91, 100, 10));
        assert!(LedgerContract::is_within_trace_window(100, 100, 10));
        // Genesis is traceable at genesis for any positive window.
        assert!(LedgerContract::is_within_trace_window(0, 0, 2_102_400));
    }

    #[test]
    fn get_transaction_state_distinguishes_absent_stub_and_full() {
        let cache = DataCache::new(false);
        let ledger = LedgerContract::new();
        let tx_hash = UInt256::from_bytes(&[9u8; 32]).unwrap();

        // Absent -> None (getTransactionHeight would return -1).
        assert!(
            ledger
                .get_transaction_state(&cache, &tx_hash)
                .unwrap()
                .is_none()
        );
        assert!(!ledger.contains_transaction(&cache, &tx_hash).unwrap());

        // Conflict stub -> Some, but `transaction` is None, so C#
        // `GetTransactionState` treats it as null and height is -1 —
        // and C# `ContainsTransaction` is false for a stub.
        cache.add(
            LedgerContract::transaction_storage_key(LedgerContract::ID, &tx_hash),
            StorageItem::from_bytes(LedgerContract::new().serialize_conflict_stub(4242).unwrap()),
        );
        let stub = ledger
            .get_transaction_state(&cache, &tx_hash)
            .unwrap()
            .unwrap();
        assert!(stub.transaction.is_none());
        assert_eq!(stub.block_index, 4242);
        assert!(
            !ledger.contains_transaction(&cache, &tx_hash).unwrap(),
            "C# ContainsTransaction must be false for a conflict stub"
        );
    }

    /// Byte-level pins of the C# `KeyBuilder` key layouts:
    /// `CreateStorageKey(Prefix_BlockHash, uint)` uses `AddBigEndian`
    /// (NativeContract.cs:403), and the transaction/conflict keys
    /// append the raw hash (and signer) bytes.
    #[test]
    fn storage_key_layouts_match_csharp_keybuilder() {
        let key = LedgerContract::block_hash_storage_key(LedgerContract::ID, 0x0102_0304);
        assert_eq!(key.id(), -4);
        assert_eq!(key.key(), &[9u8, 0x01, 0x02, 0x03, 0x04]);
        // Low indices land in the high-order byte positions.
        assert_eq!(
            LedgerContract::block_hash_storage_key(LedgerContract::ID, 7).key(),
            &[9u8, 0, 0, 0, 7]
        );

        let hash = UInt256::from_bytes(&[0xAB; 32]).unwrap();
        let mut expected = vec![11u8];
        expected.extend_from_slice(&[0xAB; 32]);
        assert_eq!(
            LedgerContract::transaction_storage_key(LedgerContract::ID, &hash).key(),
            &expected[..]
        );

        let signer = UInt160::from_bytes(&[0x11; 20]).unwrap();
        expected.extend_from_slice(&[0x11; 20]);
        assert_eq!(
            LedgerContract::conflict_signer_storage_key(LedgerContract::ID, &hash, &signer).key(),
            &expected[..]
        );

        assert_eq!(
            LedgerContract::current_block_storage_key(LedgerContract::ID).key(),
            &[12u8]
        );
        assert_eq!(
            LedgerContract::block_storage_key(LedgerContract::ID, &hash).key()[0],
            5u8
        );
    }

    /// Byte-level pins of the C# `BinarySerializer` value layouts
    /// (StackItemType: Struct = 0x41, ByteString = 0x28, Integer =
    /// 0x21; integers are minimal signed little-endian var-bytes with
    /// zero encoded as the empty span — Neo.VM `Integer`).
    #[test]
    fn value_layouts_match_csharp_binary_serializer() {
        // HashIndexState (Prefix_CurrentBlock value):
        // Struct{ ByteString(hash), Integer(index) }.
        let hash = UInt256::from_bytes(&[7u8; 32]).unwrap();
        let mut expected = vec![0x41, 0x02, 0x28, 0x20];
        expected.extend_from_slice(&[7u8; 32]);
        expected.extend_from_slice(&[0x21, 0x02, 0xD2, 0x04]); // 1234 LE
        assert_eq!(
            LedgerContract::new()
                .serialize_hash_index_state(&hash, 1234)
                .unwrap(),
            expected
        );

        // Index zero serializes as an empty Integer span.
        let mut expected = vec![0x41, 0x02, 0x28, 0x20];
        expected.extend_from_slice(&[7u8; 32]);
        expected.extend_from_slice(&[0x21, 0x00]);
        assert_eq!(
            LedgerContract::new()
                .serialize_hash_index_state(&hash, 0)
                .unwrap(),
            expected
        );

        // Conflict stub: Struct{ Integer(BlockIndex) }.
        assert_eq!(
            LedgerContract::new().serialize_conflict_stub(3).unwrap(),
            vec![0x41, 0x01, 0x21, 0x01, 0x03]
        );
        assert_eq!(
            LedgerContract::new().serialize_conflict_stub(0).unwrap(),
            vec![0x41, 0x01, 0x21, 0x00]
        );

        // Full transaction record:
        // Struct{ Integer(BlockIndex), ByteString(tx.ToArray()), Integer((byte)State) }.
        let mut tx = Transaction::new();
        tx.set_nonce(99);
        tx.set_script(vec![0x40]); // RET
        tx.set_signers(vec![neo_payloads::Signer::new(
            UInt160::from_bytes(&[0x22; 20]).unwrap(),
            neo_primitives::WitnessScope::NONE,
        )]);
        tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
        let mut writer = BinaryWriter::new();
        tx.serialize(&mut writer).unwrap();
        let tx_bytes = writer.into_bytes();
        assert!(tx_bytes.len() < 0xFD, "single-byte var-int length expected");

        let record = LedgerContract::new()
            .serialize_persisted_transaction_state(7, VMState::HALT, &tx)
            .unwrap();
        let mut expected = vec![0x41, 0x03, 0x21, 0x01, 0x07, 0x28, tx_bytes.len() as u8];
        expected.extend_from_slice(&tx_bytes);
        expected.extend_from_slice(&[0x21, 0x01, 0x01]); // HALT = 1
        assert_eq!(record, expected);

        // VMState::NONE (0) is the empty Integer span.
        let record = LedgerContract::new()
            .serialize_persisted_transaction_state(7, VMState::NONE, &tx)
            .unwrap();
        let mut expected = vec![0x41, 0x03, 0x21, 0x01, 0x07, 0x28, tx_bytes.len() as u8];
        expected.extend_from_slice(&tx_bytes);
        expected.extend_from_slice(&[0x21, 0x00]);
        assert_eq!(record, expected);

        // And the reader decodes the pinned layout back.
        let state = LedgerContract::decode_transaction_state(&record).unwrap();
        assert_eq!(state.block_index, 7);
        assert_eq!(state.state, VMState::NONE);
        let decoded_tx = state.transaction.expect("full record");
        assert_eq!(decoded_tx.nonce(), 99);
    }

    /// C# `LedgerContract.ContainsConflictHash`: the bare stub must
    /// exist, be a stub, and be traceable; then some signer stub must
    /// exist and be traceable.
    #[test]
    fn contains_conflict_hash_matches_csharp_rules() {
        let cache = DataCache::new(false);
        let ledger = LedgerContract::new();
        let hash = UInt256::from_bytes(&[0xCD; 32]).unwrap();
        let signer = UInt160::from_bytes(&[0x44; 20]).unwrap();
        let other = UInt160::from_bytes(&[0x55; 20]).unwrap();
        let mtb = 10u32;

        // Chain height 100 → traceable window is (90, 100].
        cache.add(
            LedgerContract::current_block_storage_key(LedgerContract::ID),
            StorageItem::from_bytes(
                LedgerContract::new()
                    .serialize_hash_index_state(&UInt256::from_bytes(&[1u8; 32]).unwrap(), 100)
                    .unwrap(),
            ),
        );

        // No record at all → false.
        assert!(
            !ledger
                .contains_conflict_hash(&cache, &hash, &[signer], mtb)
                .unwrap()
        );

        // Bare stub (traceable) but no signer record → false.
        cache.add(
            LedgerContract::transaction_storage_key(LedgerContract::ID, &hash),
            StorageItem::from_bytes(LedgerContract::new().serialize_conflict_stub(95).unwrap()),
        );
        assert!(
            !ledger
                .contains_conflict_hash(&cache, &hash, &[signer], mtb)
                .unwrap()
        );

        // Signer record for a different account → still false for ours…
        cache.add(
            LedgerContract::conflict_signer_storage_key(LedgerContract::ID, &hash, &other),
            StorageItem::from_bytes(LedgerContract::new().serialize_conflict_stub(95).unwrap()),
        );
        assert!(
            !ledger
                .contains_conflict_hash(&cache, &hash, &[signer], mtb)
                .unwrap()
        );
        // …and true for the matching one.
        assert!(
            ledger
                .contains_conflict_hash(&cache, &hash, &[other], mtb)
                .unwrap()
        );

        // An untraceable signer record (95 - window) does not count.
        cache.add(
            LedgerContract::conflict_signer_storage_key(LedgerContract::ID, &hash, &signer),
            StorageItem::from_bytes(LedgerContract::new().serialize_conflict_stub(80).unwrap()),
        );
        assert!(
            !ledger
                .contains_conflict_hash(&cache, &hash, &[signer], mtb)
                .unwrap()
        );

        // A full transaction record under the hash is NOT a conflict
        // record (C#: `stub.Transaction is not null` → false).
        let mut tx = Transaction::new();
        tx.set_script(vec![0x40]);
        tx.set_signers(vec![neo_payloads::Signer::new(
            other,
            neo_primitives::WitnessScope::NONE,
        )]);
        tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
        cache.update(
            LedgerContract::transaction_storage_key(LedgerContract::ID, &hash),
            StorageItem::from_bytes(
                LedgerContract::new()
                    .serialize_persisted_transaction_state(95, VMState::HALT, &tx)
                    .unwrap(),
            ),
        );
        assert!(
            !ledger
                .contains_conflict_hash(&cache, &hash, &[other], mtb)
                .unwrap()
        );
    }

    #[test]
    fn current_index_and_hash_round_trip_through_storage() {
        let cache = DataCache::new(false);
        let ledger = LedgerContract::new();

        // Empty ledger: index 0, zero hash (C# returns these when the
        // current-block pointer is absent).
        assert_eq!(ledger.current_index(&cache).unwrap(), 0);
        assert_eq!(ledger.current_hash(&cache).unwrap(), UInt256::default());

        // Write a HashIndexState under the current-block key (prefix 12) and
        // read it back, exercising the exact on-disk format the engine uses.
        let hash = UInt256::from_bytes(&[7u8; 32]).unwrap();
        let bytes = LedgerContract::new()
            .serialize_hash_index_state(&hash, 1234)
            .unwrap();
        cache.add(
            LedgerContract::current_block_storage_key(LedgerContract::ID),
            StorageItem::from_bytes(bytes),
        );
        assert_eq!(ledger.current_index(&cache).unwrap(), 1234);
        assert_eq!(ledger.current_hash(&cache).unwrap(), hash);
    }
}
