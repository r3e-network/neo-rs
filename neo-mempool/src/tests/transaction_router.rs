use super::*;
use neo_payloads::{Signer, Transaction, Witness};
use neo_primitives::{UInt160, WitnessScope};
use neo_vm_rs::OpCode;

fn sample_tx() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(0);
    tx.set_network_fee(1);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

#[test]
fn preverify_accepts_well_formed_transaction() {
    let router = TransactionRouter::new(ProtocolSettings::default());
    let result = router.preverify(sample_tx(), true);
    assert!(result.is_success());
    assert!(result.relay);
}
