//! LedgerContract block/transaction record writes.
//!
//! C# `LedgerContract.OnPersistAsync` (LedgerContract.cs:42) writes, for
//! every persisted block:
//!
//! - `Prefix_BlockHash (9)` + block index → the block hash;
//! - `Prefix_Block (5)` + block hash → the serialized `TrimmedBlock`
//!   (header + transaction hashes);
//! - `Prefix_Transaction (11)` + tx hash → the `TransactionState`
//!   record (block index, VM state, full transaction), overwriting any
//!   previously-stored malicious conflict stub;
//! - `Prefix_Transaction (11)` + conflict hash (and + each signer) →
//!   conflict-stub records for every `Conflicts` attribute.
//!
//! and `LedgerContract.PostPersistAsync` (LedgerContract.cs:77) updates
//! `Prefix_CurrentBlock (12)` → the `(hash, index)` pointer.
//!
//! The Rust `neo_native_contracts::LedgerContract` is read-only (its
//! `on_persist` hook is the trait default no-op) and exposes no writer,
//! so the persist pipeline performs these writes itself, *through the
//! ledger crate's public record codec* (`serialize_hash_index_state`,
//! `serialize_persisted_transaction_state`, `serialize_conflict_stub`)
//! so the bytes written here are exactly the bytes its readers parse.
//!
//! ## Byte layouts (C#-exact)
//!
//! Every key and value matches C# `LedgerContract` byte-for-byte:
//!
//! - `Prefix_BlockHash (9)` keys carry a **big-endian** block index
//!   (`CreateStorageKey(prefix, uint bigEndianKey)` →
//!   `KeyBuilder.AddBigEndian`, NativeContract.cs:403); the value is
//!   the raw 32-byte block hash (`PersistingBlock.Hash.ToArray()`);
//! - `Prefix_Block (5)` values are `TrimmedBlock.ToArray()` — the
//!   `ISerializable` header followed by the var-array of tx hashes;
//! - `Prefix_Transaction (11)` values are the `BinarySerializer` form
//!   of the interoperable `TransactionState` stack item:
//!   `Struct[Integer(BlockIndex), ByteString(tx bytes),
//!   Integer((byte)State)]` for full records and
//!   `Struct[Integer(BlockIndex)]` for conflict stubs;
//! - `Prefix_CurrentBlock (12)` values are the `BinarySerializer` form
//!   of the interoperable `HashIndexState` stack item:
//!   `Struct[ByteString(hash), Integer(index)]`.

use neo_error::{CoreError, CoreResult};
use neo_io::Serializable;
use neo_native_contracts::LedgerContract;
use neo_payloads::{Block, Transaction, TransactionAttribute};
use neo_primitives::{UInt160, UInt256};
use neo_storage::{DataCache, StorageItem, StorageKey};
use neo_vm_rs::VmState as VMState;

/// C# `LedgerContract.Prefix_BlockHash` (9).
const PREFIX_BLOCK_HASH: u8 = 9;
/// C# `LedgerContract.Prefix_Block` (5).
const PREFIX_BLOCK: u8 = 5;
/// C# `LedgerContract.Prefix_Transaction` (11).
const PREFIX_TRANSACTION: u8 = 11;
/// C# `LedgerContract.Prefix_CurrentBlock` (12).
const PREFIX_CURRENT_BLOCK: u8 = 12;

/// Upsert helper: C# `GetAndChange(key, factory).FromReplica(item)`
/// replaces the stored value whether or not the key already exists.
fn upsert(cache: &DataCache, key: StorageKey, value: Vec<u8>) {
    if cache.get(&key).is_some() {
        cache.update(key, StorageItem::from_bytes(value));
    } else {
        cache.add(key, StorageItem::from_bytes(value));
    }
}

/// LedgerContract block/transaction record key builders and writers.
///
/// The cohesive `LedgerContract.OnPersistAsync` / `PostPersistAsync` write
/// set grouped onto a single zero-sized type: each key builder and writer is
/// an associated function (none carry state), so callers spell them
/// `LedgerRecords::*`.
pub(crate) struct LedgerRecords;

impl LedgerRecords {
    /// `Prefix_BlockHash` key: prefix + **big-endian** block index, the C#
    /// `CreateStorageKey(Prefix_BlockHash, engine.PersistingBlock.Index)`
    /// overload (`KeyBuilder.AddBigEndian(uint)`, NativeContract.cs:403).
    fn block_hash_key(index: u32) -> StorageKey {
        let mut key = Vec::with_capacity(5);
        key.push(PREFIX_BLOCK_HASH);
        key.extend_from_slice(&index.to_be_bytes());
        StorageKey::new(LedgerContract::ID, key)
    }

    /// `Prefix_Block` key (prefix + 32-byte block hash).
    fn block_key(hash: &UInt256) -> StorageKey {
        let mut key = Vec::with_capacity(33);
        key.push(PREFIX_BLOCK);
        key.extend_from_slice(&hash.to_bytes());
        StorageKey::new(LedgerContract::ID, key)
    }

    /// `Prefix_Transaction` key (prefix + 32-byte transaction hash).
    fn transaction_key(hash: &UInt256) -> StorageKey {
        let mut key = Vec::with_capacity(33);
        key.push(PREFIX_TRANSACTION);
        key.extend_from_slice(&hash.to_bytes());
        StorageKey::new(LedgerContract::ID, key)
    }

    /// `Prefix_Transaction` conflict key (prefix + conflict hash + signer),
    /// mirroring C# `CreateStorageKey(Prefix_Transaction, attr.Hash, signer)`.
    fn conflict_signer_key(hash: &UInt256, signer: &UInt160) -> StorageKey {
        let mut key = Vec::with_capacity(53);
        key.push(PREFIX_TRANSACTION);
        key.extend_from_slice(&hash.to_bytes());
        key.extend_from_slice(&signer.to_bytes());
        StorageKey::new(LedgerContract::ID, key)
    }

    /// `Prefix_CurrentBlock` key.
    fn current_block_key() -> StorageKey {
        StorageKey::new(LedgerContract::ID, vec![PREFIX_CURRENT_BLOCK])
    }

    /// Serializes the `TrimmedBlock` bytes persisted under `Prefix_Block`.
    ///
    /// This is intentionally byte-for-byte the same as
    /// `TrimmedBlock::new(block.header.clone(), hashes.to_vec()).serialize(...)`,
    /// but it avoids allocating a temporary `TrimmedBlock` and hash vector in
    /// the empty-block fast-forward path, where the ledger still has to persist
    /// one trimmed block per height.
    fn serialize_trimmed_block(block: &Block, hashes: &[UInt256]) -> CoreResult<Vec<u8>> {
        let mut writer = neo_io::BinaryWriter::with_capacity(
            <neo_payloads::Header as Serializable>::size(&block.header)
                + neo_io::serializable::helper::SerializeHelper::get_var_size_serializable_slice(
                    hashes,
                ),
        );
        <neo_payloads::Header as Serializable>::serialize(&block.header, &mut writer)
            .map_err(|e| CoreError::serialization(format!("ledger records: block header: {e}")))?;
        writer
            .write_var_int(hashes.len() as u64)
            .map_err(|e| CoreError::serialization(format!("ledger records: hash count: {e}")))?;
        for hash in hashes {
            <UInt256 as Serializable>::serialize(hash, &mut writer)
                .map_err(|e| CoreError::serialization(format!("ledger records: tx hash: {e}")))?;
        }
        Ok(writer.into_bytes())
    }

    /// The exact write set of C# `LedgerContract.OnPersistAsync`: the
    /// block-hash index entry, the trimmed block, the per-transaction
    /// records (initial `VMState::NONE`, like C#'s `State = VMState.NONE`),
    /// and the conflict stubs for every `Conflicts` attribute (one stub per
    /// conflict hash plus one per conflicting signer).
    pub(crate) fn write_on_persist_records(
        cache: &DataCache,
        block: &Block,
        block_hash: &UInt256,
    ) -> CoreResult<()> {
        let index = block.index();

        // Prefix_BlockHash: index → hash (raw 32 bytes, like C#
        // `engine.PersistingBlock.Hash.ToArray()`).
        cache.add(
            Self::block_hash_key(index),
            StorageItem::from_bytes(block_hash.to_bytes()),
        );

        // Prefix_Block: hash → TrimmedBlock.Create(block).ToArray().
        let mut tx_hashes = Vec::with_capacity(block.transactions.len());
        for tx in &block.transactions {
            tx_hashes.push(tx.try_hash().map_err(|e| {
                CoreError::invalid_operation(format!("ledger records: tx hash: {e}"))
            })?);
        }
        cache.add(
            Self::block_key(block_hash),
            StorageItem::from_bytes(Self::serialize_trimmed_block(block, &tx_hashes)?),
        );

        // Per-transaction records + conflict stubs, in block order (later
        // writes overwrite earlier ones, exactly like the C# loop).
        for (tx, tx_hash) in block.transactions.iter().zip(tx_hashes.iter()) {
            let record = LedgerContract::new().serialize_persisted_transaction_state(
                index,
                VMState::NONE,
                tx,
            )?;
            upsert(cache, Self::transaction_key(tx_hash), record);

            let conflict_hashes: Vec<UInt256> = tx
                .attributes()
                .iter()
                .filter_map(|attr| match attr {
                    TransactionAttribute::Conflicts(c) => Some(c.hash),
                    _ => None,
                })
                .collect();
            if conflict_hashes.is_empty() {
                continue;
            }
            let signers: Vec<UInt160> = tx.signers().iter().map(|s| s.account).collect();
            let stub = LedgerContract::new().serialize_conflict_stub(index)?;
            for conflict_hash in &conflict_hashes {
                upsert(cache, Self::transaction_key(conflict_hash), stub.clone());
                for signer in &signers {
                    upsert(
                        cache,
                        Self::conflict_signer_key(conflict_hash, signer),
                        stub.clone(),
                    );
                }
            }
        }

        Ok(())
    }

    /// Rewrites a transaction's `Prefix_Transaction` record with its final
    /// VM state. C# mutates the in-memory `TransactionState` stored by
    /// `OnPersistAsync` (`transactionState.State = engine.Execute()`), so
    /// the record committed at the end of `Blockchain.Persist` carries the
    /// execution result; the explicit Rust codec requires a rewrite.
    pub(crate) fn update_transaction_vm_state(
        cache: &DataCache,
        block_index: u32,
        tx: &Transaction,
        tx_hash: &UInt256,
        vm_state: VMState,
    ) -> CoreResult<()> {
        let record = LedgerContract::new().serialize_persisted_transaction_state(
            block_index,
            vm_state,
            tx,
        )?;
        upsert(cache, Self::transaction_key(tx_hash), record);
        Ok(())
    }

    /// The exact write of C# `LedgerContract.PostPersistAsync`: the
    /// `Prefix_CurrentBlock` pointer becomes `(block hash, block index)`.
    pub(crate) fn write_post_persist_record(
        cache: &DataCache,
        block_hash: &UInt256,
        index: u32,
    ) -> CoreResult<()> {
        let value = LedgerContract::new().serialize_hash_index_state(block_hash, index)?;
        upsert(cache, Self::current_block_key(), value);
        Ok(())
    }
}

#[cfg(test)]
#[path = "../tests/ledger/ledger_records.rs"]
mod tests;
