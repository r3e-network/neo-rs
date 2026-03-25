use neo_core::ledger::{BlockchainCommand, Import};
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::{block::Block, header::Header};
use neo_core::{KeyPair, Signer, Transaction, UInt160, UInt256, Witness, WitnessScope};
use neo_core::{NeoSystem, ProtocolSettings};
use tokio::time::{sleep, Duration};

fn build_unfunded_signed_transaction(
    settings: &ProtocolSettings,
    keypair: &KeyPair,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_network_fee(1_0000_0000);
    tx.set_system_fee(30);
    tx.set_valid_until_block(10);
    tx.set_script(vec![0x11]); // PUSH1
    tx.set_signers(vec![Signer::new(
        keypair.get_script_hash(),
        WitnessScope::GLOBAL,
    )]);

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let signature = keypair.sign(&sign_data).expect("sign");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(0x0c); // PUSHDATA1
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        keypair.get_verification_script(),
    )]);
    tx
}

fn build_fee_burn_failure_block(system: &NeoSystem, settings: &ProtocolSettings) -> Block {
    let keypair = KeyPair::generate().expect("keypair");
    let tx = build_unfunded_signed_transaction(settings, &keypair);

    let mut block = Block::new();
    let mut genesis = system.genesis_block().as_ref().clone();
    let prev_hash = genesis.hash();

    let mut header = Header::new();
    header.set_index(1);
    header.set_prev_hash(prev_hash);
    header.set_merkle_root(UInt256::zero());
    header.set_next_consensus(UInt160::zero());
    header.set_timestamp(1);
    header.witness = Witness::new();

    block.header = header;
    block.transactions = vec![tx];
    block.rebuild_merkle_root();
    block
}

#[tokio::test(flavor = "multi_thread")]
async fn persist_block_reports_fee_burn_failure_for_unfunded_sender() {
    let settings = ProtocolSettings::mainnet();
    let system = NeoSystem::new(settings.clone(), None, None).expect("NeoSystem::new");
    let block = build_fee_burn_failure_block(&system, &settings);

    let err = match system.persist_block(block) {
        Ok(_) => panic!("persist should fail when sender cannot pay GAS fees"),
        Err(err) => err,
    };
    let message = err.to_string();
    assert!(
        message.contains("GasToken burn failed")
            || message.contains("Insufficient balance for burn"),
        "unexpected persist error: {message}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn import_does_not_advance_height_when_persist_fails() {
    let settings = ProtocolSettings::mainnet();
    let system = NeoSystem::new(settings, None, None).expect("NeoSystem::new");
    let blockchain = system.blockchain_actor();
    let block = build_fee_burn_failure_block(&system, &ProtocolSettings::mainnet());

    let import = Import {
        blocks: vec![block],
        verify: false,
    };

    blockchain
        .tell(BlockchainCommand::Import(import))
        .expect("send failed");

    sleep(Duration::from_millis(100)).await;

    let height = system.ledger_context().current_height();
    assert_eq!(
        height, 0,
        "failed persistence during import must not advance ledger height",
    );
}
