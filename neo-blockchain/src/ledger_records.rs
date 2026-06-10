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
//! ## Known C# byte-layout divergences (owned by the ledger crate)
//!
//! The record *values* and one key layout intentionally follow the Rust
//! `LedgerContract` reader rather than C#, because the reader is in a
//! crate this pipeline cannot change and a self-consistent store is a
//! prerequisite for everything downstream:
//!
//! - `Prefix_BlockHash` keys: the Rust reader uses a **little-endian**
//!   index (`block_hash_storage_key`), C# `CreateStorageKey(prefix,
//!   uint bigEndianKey)` uses **big-endian**;
//! - `Prefix_Transaction` values: the Rust codec is a tagged binary
//!   record (kind/index/state/tx bytes), C# stores the interoperable
//!   stack-item serialization of `TransactionState`;
//! - `Prefix_CurrentBlock` values: raw `hash ‖ u32le`, C# stores the
//!   interoperable `HashIndexState` stack item.
//!
//! Until the ledger crate's reader and this writer migrate together,
//! the storage *state root* for these records differs from C# mainnet.

use neo_data_cache::{DataCache, StorageItem, StorageKey};
use neo_error::{CoreError, CoreResult};
use neo_io::Serializable;
use neo_native_contracts::ledger_contract::{
    serialize_conflict_stub, serialize_hash_index_state, serialize_persisted_transaction_state,
};
use neo_native_contracts::LedgerContract;
use neo_payloads::{Block, Transaction, TransactionAttribute, TrimmedBlock};
use neo_primitives::{UInt160, UInt256};
use neo_vm_rs::VmState as VMState;

/// C# `LedgerContract.Prefix_BlockHash` (9).
const PREFIX_BLOCK_HASH: u8 = 9;
/// C# `LedgerContract.Prefix_Block` (5).
const PREFIX_BLOCK: u8 = 5;
/// C# `LedgerContract.Prefix_Transaction` (11).
const PREFIX_TRANSACTION: u8 = 11;
/// C# `LedgerContract.Prefix_CurrentBlock` (12).
const PREFIX_CURRENT_BLOCK: u8 = 12;

/// `Prefix_BlockHash` key. Little-endian index to match the Rust
/// `LedgerContract::get_block_hash` reader (C# uses big-endian — see the
/// module docs).
fn block_hash_key(index: u32) -> StorageKey {
    let mut key = Vec::with_capacity(5);
    key.push(PREFIX_BLOCK_HASH);
    key.extend_from_slice(&index.to_le_bytes());
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

/// Upsert helper: C# `GetAndChange(key, factory).FromReplica(item)`
/// replaces the stored value whether or not the key already exists.
fn upsert(cache: &DataCache, key: StorageKey, value: Vec<u8>) {
    if cache.get(&key).is_some() {
        cache.update(key, StorageItem::from_bytes(value));
    } else {
        cache.add(key, StorageItem::from_bytes(value));
    }
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
        block_hash_key(index),
        StorageItem::from_bytes(block_hash.to_bytes()),
    );

    // Prefix_Block: hash → TrimmedBlock.Create(block).ToArray().
    let mut tx_hashes = Vec::with_capacity(block.transactions.len());
    for tx in &block.transactions {
        tx_hashes.push(tx.try_hash().map_err(|e| {
            CoreError::invalid_operation(format!("ledger records: tx hash: {e}"))
        })?);
    }
    let trimmed = TrimmedBlock::new(block.header.clone(), tx_hashes.clone());
    let mut writer = neo_io::BinaryWriter::new();
    trimmed
        .serialize(&mut writer)
        .map_err(|e| CoreError::serialization(format!("ledger records: trimmed block: {e}")))?;
    cache.add(block_key(block_hash), StorageItem::from_bytes(writer.into_bytes()));

    // Per-transaction records + conflict stubs, in block order (later
    // writes overwrite earlier ones, exactly like the C# loop).
    for (tx, tx_hash) in block.transactions.iter().zip(tx_hashes.iter()) {
        let record = serialize_persisted_transaction_state(index, VMState::NONE, tx)?;
        upsert(cache, transaction_key(tx_hash), record);

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
        let stub = serialize_conflict_stub(index)?;
        for conflict_hash in &conflict_hashes {
            upsert(cache, transaction_key(conflict_hash), stub.clone());
            for signer in &signers {
                upsert(cache, conflict_signer_key(conflict_hash, signer), stub.clone());
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
    let record = serialize_persisted_transaction_state(block_index, vm_state, tx)?;
    upsert(cache, transaction_key(tx_hash), record);
    Ok(())
}

/// The exact write of C# `LedgerContract.PostPersistAsync`: the
/// `Prefix_CurrentBlock` pointer becomes `(block hash, block index)`.
pub(crate) fn write_post_persist_record(
    cache: &DataCache,
    block_hash: &UInt256,
    index: u32,
) -> CoreResult<()> {
    let value = serialize_hash_index_state(block_hash, index)?;
    upsert(cache, current_block_key(), value);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_payloads::Header;

    /// The keys built here must parse through the Rust `LedgerContract`
    /// readers — that is the whole point of writing via its codec.
    #[test]
    fn records_round_trip_through_the_ledger_contract_readers() {
        let cache = DataCache::new(false);
        let ledger = LedgerContract::new();

        let mut header = Header::new();
        header.set_index(7);
        let mut tx = Transaction::new();
        tx.set_nonce(99);
        tx.set_script(vec![0x40]); // RET
        tx.set_signers(vec![neo_payloads::Signer::new(
            UInt160::from_bytes(&[0x22; 20]).unwrap(),
            neo_primitives::WitnessScope::NONE,
        )]);
        tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
        let tx_hash = tx.try_hash().expect("tx hash");
        let block = Block::from_parts(header, vec![tx.clone()]);
        let block_hash = block.header.try_hash().expect("block hash");

        write_on_persist_records(&cache, &block, &block_hash).expect("on-persist records");
        write_post_persist_record(&cache, &block_hash, 7).expect("post-persist record");

        // Block-hash index + trimmed block.
        assert_eq!(
            ledger.get_block_hash(&cache, 7).expect("get_block_hash"),
            Some(block_hash)
        );
        let trimmed = ledger
            .get_trimmed_block(&cache, &block_hash)
            .expect("get_trimmed_block")
            .expect("trimmed block present");
        assert_eq!(trimmed.header.index(), 7);
        assert_eq!(trimmed.hashes, vec![tx_hash]);

        // Transaction record: initial NONE state, then the post-execution
        // rewrite flips it to HALT.
        let state = ledger
            .get_transaction_state(&cache, &tx_hash)
            .expect("get_transaction_state")
            .expect("record present");
        assert_eq!(state.block_index, 7);
        assert_eq!(state.state, VMState::NONE);
        update_transaction_vm_state(&cache, 7, &tx, &tx_hash, VMState::HALT)
            .expect("vm-state rewrite");
        let state = ledger
            .get_transaction_state(&cache, &tx_hash)
            .expect("get_transaction_state")
            .expect("record present");
        assert_eq!(state.state, VMState::HALT);

        // Current-block pointer.
        assert_eq!(ledger.current_index(&cache).expect("current_index"), 7);
        assert_eq!(ledger.current_hash(&cache).expect("current_hash"), block_hash);
    }

    /// C# stores conflict stubs under the bare conflict hash and under
    /// hash‖signer; the bare-hash stub must read back as a record whose
    /// `transaction` is `None`.
    #[test]
    fn conflict_attributes_write_stub_records() {
        let cache = DataCache::new(false);
        let ledger = LedgerContract::new();

        let conflict_hash = UInt256::from_bytes(&[0xAB; 32]).unwrap();
        let signer_account = UInt160::from_bytes(&[0x11; 20]).unwrap();
        let mut tx = Transaction::new();
        tx.set_script(vec![0x40]);
        tx.set_signers(vec![neo_payloads::Signer::new(
            signer_account,
            neo_primitives::WitnessScope::NONE,
        )]);
        tx.set_attributes(vec![TransactionAttribute::Conflicts(
            neo_payloads::Conflicts::new(conflict_hash),
        )]);
        tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
        let mut header = Header::new();
        header.set_index(3);
        let block = Block::from_parts(header, vec![tx]);
        let block_hash = block.header.try_hash().unwrap();

        write_on_persist_records(&cache, &block, &block_hash).expect("records");

        let stub = ledger
            .get_transaction_state(&cache, &conflict_hash)
            .expect("read stub")
            .expect("stub present");
        assert!(stub.transaction.is_none(), "conflict stub has no transaction");
        assert_eq!(stub.block_index, 3);

        // The signer-suffixed stub exists with the same payload.
        let key = conflict_signer_key(&conflict_hash, &signer_account);
        let raw = cache.get(&key).expect("signer stub present");
        assert_eq!(
            raw.value_bytes().into_owned(),
            serialize_conflict_stub(3).unwrap()
        );
    }
}
