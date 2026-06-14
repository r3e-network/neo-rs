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
use neo_payloads::{Block, Transaction, TransactionAttribute, TrimmedBlock};
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
        let trimmed = TrimmedBlock::new(block.header.clone(), tx_hashes.clone());
        let mut writer = neo_io::BinaryWriter::new();
        trimmed
            .serialize(&mut writer)
            .map_err(|e| CoreError::serialization(format!("ledger records: trimmed block: {e}")))?;
        cache.add(
            Self::block_key(block_hash),
            StorageItem::from_bytes(writer.into_bytes()),
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

        LedgerRecords::write_on_persist_records(&cache, &block, &block_hash)
            .expect("on-persist records");
        LedgerRecords::write_post_persist_record(&cache, &block_hash, 7)
            .expect("post-persist record");

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
        LedgerRecords::update_transaction_vm_state(&cache, 7, &tx, &tx_hash, VMState::HALT)
            .expect("vm-state rewrite");
        let state = ledger
            .get_transaction_state(&cache, &tx_hash)
            .expect("get_transaction_state")
            .expect("record present");
        assert_eq!(state.state, VMState::HALT);

        // Current-block pointer.
        assert_eq!(ledger.current_index(&cache).expect("current_index"), 7);
        assert_eq!(
            ledger.current_hash(&cache).expect("current_hash"),
            block_hash
        );
    }

    /// Byte-level pin of the C# `LedgerContract.OnPersistAsync` /
    /// `PostPersistAsync` write set: every key (prefix + big-endian
    /// index / raw hash) and every value (raw hash bytes, TrimmedBlock
    /// `ISerializable` bytes, `BinarySerializer` stack-item records)
    /// is asserted against independently assembled C# layouts — not
    /// just round-tripped through the Rust codec.
    #[test]
    fn persisted_records_pin_csharp_key_and_value_bytes() {
        let cache = DataCache::new(false);

        let mut header = Header::new();
        header.set_index(0x0102_0304);
        let mut tx = Transaction::new();
        tx.set_nonce(7);
        tx.set_script(vec![0x40]); // RET
        tx.set_signers(vec![neo_payloads::Signer::new(
            UInt160::from_bytes(&[0x22; 20]).unwrap(),
            neo_primitives::WitnessScope::NONE,
        )]);
        tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
        let tx_hash = tx.try_hash().unwrap();
        let block = Block::from_parts(header, vec![tx.clone()]);
        let block_hash = block.header.try_hash().unwrap();

        LedgerRecords::write_on_persist_records(&cache, &block, &block_hash)
            .expect("on-persist records");
        LedgerRecords::write_post_persist_record(&cache, &block_hash, 0x0102_0304)
            .expect("post-persist");

        let raw = |key: &StorageKey| {
            cache
                .get(key)
                .map(|item| item.value_bytes().into_owned())
                .expect("record present")
        };

        // --- Prefix_BlockHash (9): key = prefix ‖ BIG-ENDIAN index;
        // value = the raw 32-byte block hash.
        let bh_key = LedgerRecords::block_hash_key(0x0102_0304);
        assert_eq!(bh_key.id(), LedgerContract::ID);
        assert_eq!(bh_key.key(), &[9u8, 0x01, 0x02, 0x03, 0x04]);
        assert_eq!(raw(&bh_key), block_hash.to_bytes());

        // --- Prefix_Block (5): key = prefix ‖ hash; value =
        // TrimmedBlock.ToArray() = header bytes ‖ var-int count ‖ hashes.
        let b_key = LedgerRecords::block_key(&block_hash);
        let mut expected_key = vec![5u8];
        expected_key.extend_from_slice(&block_hash.to_bytes());
        assert_eq!(b_key.key(), &expected_key[..]);
        let mut header_writer = neo_io::BinaryWriter::new();
        Serializable::serialize(&block.header, &mut header_writer).unwrap();
        let mut expected_block = header_writer.into_bytes();
        expected_block.push(1); // var-int tx count
        expected_block.extend_from_slice(&tx_hash.to_bytes());
        assert_eq!(raw(&b_key), expected_block);

        // --- Prefix_Transaction (11): value = BinarySerializer bytes of
        // Struct[Integer(BlockIndex), ByteString(tx bytes), Integer(state)]
        // (0x41 Struct, 0x21 Integer, 0x28 ByteString; VMState.NONE = 0
        // serializes as the empty Integer span).
        let mut tx_writer = neo_io::BinaryWriter::new();
        Serializable::serialize(&tx, &mut tx_writer).unwrap();
        let tx_bytes = tx_writer.into_bytes();
        assert!(tx_bytes.len() < 0xFD);
        let mut expected_record = vec![
            0x41,
            0x03,
            0x21,
            0x04,
            0x04,
            0x03,
            0x02,
            0x01, // Integer 0x01020304 LE
            0x28,
            tx_bytes.len() as u8,
        ];
        expected_record.extend_from_slice(&tx_bytes);
        expected_record.extend_from_slice(&[0x21, 0x00]); // VMState::NONE
        assert_eq!(
            raw(&LedgerRecords::transaction_key(&tx_hash)),
            expected_record
        );

        // After execution the record is rewritten with HALT (= 1).
        LedgerRecords::update_transaction_vm_state(
            &cache,
            0x0102_0304,
            &tx,
            &tx_hash,
            VMState::HALT,
        )
        .unwrap();
        let mut expected_halt = vec![
            0x41,
            0x03,
            0x21,
            0x04,
            0x04,
            0x03,
            0x02,
            0x01,
            0x28,
            tx_bytes.len() as u8,
        ];
        expected_halt.extend_from_slice(&tx_bytes);
        expected_halt.extend_from_slice(&[0x21, 0x01, 0x01]);
        assert_eq!(
            raw(&LedgerRecords::transaction_key(&tx_hash)),
            expected_halt
        );

        // --- Prefix_CurrentBlock (12): value = BinarySerializer bytes of
        // Struct[ByteString(hash), Integer(index)] (HashIndexState).
        assert_eq!(LedgerRecords::current_block_key().key(), &[12u8]);
        let mut expected_pointer = vec![0x41, 0x02, 0x28, 0x20];
        expected_pointer.extend_from_slice(&block_hash.to_bytes());
        expected_pointer.extend_from_slice(&[0x21, 0x04, 0x04, 0x03, 0x02, 0x01]);
        assert_eq!(raw(&LedgerRecords::current_block_key()), expected_pointer);
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

        LedgerRecords::write_on_persist_records(&cache, &block, &block_hash).expect("records");

        let stub = ledger
            .get_transaction_state(&cache, &conflict_hash)
            .expect("read stub")
            .expect("stub present");
        assert!(
            stub.transaction.is_none(),
            "conflict stub has no transaction"
        );
        assert_eq!(stub.block_index, 3);

        // The signer-suffixed stub exists with the same payload.
        let key = LedgerRecords::conflict_signer_key(&conflict_hash, &signer_account);
        let raw = cache.get(&key).expect("signer stub present");
        assert_eq!(
            raw.value_bytes().into_owned(),
            LedgerContract::new().serialize_conflict_stub(3).unwrap()
        );
    }
}
