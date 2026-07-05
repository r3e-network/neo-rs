use super::{MerkleBlockPayload, pack_wire_flags, pad_flags};
use crate::Witness;
use crate::block::Block;
use crate::signer::Signer;
use crate::transaction::Transaction;
use neo_primitives::{UInt160, WitnessScope};
use neo_vm_rs::OpCode;

fn transaction_with_script(script: Vec<u8>) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_attributes(Vec::new());
    tx.set_script(script);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

#[test]
fn pad_flags_single_depth_adds_placeholder() {
    let padded = pad_flags(Vec::new(), 1);
    assert_eq!(padded, vec![false]);

    let padded = pad_flags(vec![true], 1);
    assert_eq!(padded, vec![true]);
}

#[test]
fn pad_flags_extends_and_truncates_to_width() {
    // Depth 3 => 4 leaves
    let padded = pad_flags(vec![true], 3);
    assert_eq!(padded, vec![true, false, false, false]);

    let padded = pad_flags(vec![true, true, true, true, true], 3);
    assert_eq!(padded, vec![true, true, true, true]);
}

#[test]
fn pack_wire_flags_matches_csharp_bitarray_copyto() {
    // C# `MerkleBlockPayload.Create`:
    //   byte[] buffer = new byte[(flags.Length + 7) / 8];  // flags.Length == TxCount
    //   flags.CopyTo(buffer, 0);                            // LSB-first per byte
    //
    // 3 transactions, only tx0 matched => 1 byte, bit0 set.
    assert_eq!(pack_wire_flags(&[true, false, false], 3), vec![0b0000_0001]);

    // 8 transactions, tx0 + tx7 matched => exactly 1 byte, bits 0 and 7 set.
    assert_eq!(
        pack_wire_flags(&[true, false, false, false, false, false, false, true], 8),
        vec![0b1000_0001]
    );

    // 9 transactions => ceil(9/8) == 2 bytes. tx0 and tx8 matched.
    // Byte0 bit0 => 0x01, byte1 bit0 (== tx8) => 0x01.
    assert_eq!(
        pack_wire_flags(
            &[true, false, false, false, false, false, false, false, true],
            9
        ),
        vec![0b0000_0001, 0b0000_0001]
    );

    // 17 transactions => ceil(17/8) == 3 bytes (regression guard against the old
    // 2^(depth-1)-padded packing, which produced 4 bytes for 17 leaves).
    let mut seventeen = vec![false; 17];
    seventeen[0] = true;
    seventeen[16] = true;
    assert_eq!(
        pack_wire_flags(&seventeen, 17),
        vec![0b0000_0001, 0b0000_0000, 0b0000_0001]
    );

    // Short input is zero-extended up to TxCount (C# BitArray of length TxCount).
    assert_eq!(pack_wire_flags(&[true], 3), vec![0b0000_0001]);
    // No transactions => zero bytes.
    assert_eq!(pack_wire_flags(&[], 0), Vec::<u8>::new());
}

#[test]
fn create_flags_length_is_ceil_tx_count_over_8() {
    // Build a block with 9 transactions so ceil(9/8) == 2 bytes on the wire,
    // and assert the created payload does not pad to the tree width.
    let mut block = Block::new();
    for i in 0..9u8 {
        block
            .transactions
            .push(transaction_with_script(vec![OpCode::PUSH1.byte(), i]));
    }

    // Match only the first transaction.
    let mut filter = vec![false; 9];
    filter[0] = true;

    let payload = MerkleBlockPayload::try_create(&mut block, filter).unwrap();

    assert_eq!(payload.tx_count, 9);
    // ceil(9 / 8) == 2 — NOT ceil(2^(depth-1) / 8).
    assert_eq!(payload.flags.len(), 2);
    // tx0 matched => byte0 bit0 set; remaining bits false.
    assert_eq!(payload.flags, vec![0b0000_0001, 0b0000_0000]);
}

#[test]
fn try_create_rejects_unserializable_transaction_hash() {
    let mut block = Block::new();
    block.transactions.push(transaction_with_script(vec![
        OpCode::NOP.byte();
        u16::MAX as usize + 1
    ]));

    assert!(MerkleBlockPayload::try_create(&mut block, vec![true]).is_err());
}

#[test]
fn try_create_matches_legacy_create_for_valid_block() {
    let mut block = Block::new();
    block
        .transactions
        .push(transaction_with_script(vec![OpCode::PUSH1.byte()]));
    let mut legacy_block = block.clone();

    let fallible = MerkleBlockPayload::try_create(&mut block, vec![true]).unwrap();
    let legacy = MerkleBlockPayload::create(&mut legacy_block, vec![true]);

    assert_eq!(fallible.hashes, legacy.hashes);
    assert_eq!(fallible.flags, legacy.flags);
    assert_eq!(fallible.tx_count, legacy.tx_count);
}
