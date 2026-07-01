use super::{MerkleBlockPayload, pack_flags, pad_flags};
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
fn pack_flags_uses_neo_lsb_first_byte_order() {
    let packed = pack_flags(&[true, false, true, true, false, false, false, false, true]);

    assert_eq!(packed, vec![0b0000_1101, 0b0000_0001]);
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
