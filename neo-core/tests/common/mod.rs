use neo_core::network::p2p::payloads::{signer::Signer, witness::Witness};
use neo_core::{Transaction, UInt160, WitnessScope};

pub fn test_transaction() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x01020304);
    tx.set_system_fee(100_000_000);
    tx.set_network_fee(1);
    tx.set_valid_until_block(0x01020304);
    tx.set_script(vec![0x11]);
    tx.set_signers(vec![Signer::new(
        UInt160::zero(),
        WitnessScope::CALLED_BY_ENTRY,
    )]);
    tx.set_attributes(Vec::new());
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

pub fn test_byte_array(size: usize, fill_byte: u8) -> Vec<u8> {
    vec![fill_byte; size]
}
