#![cfg(feature = "runtime")]

use neo_core::ledger::{Block, BlockHeader};
use neo_core::neo_ledger::ApplicationExecuted;
use neo_core::neo_system::NeoSystem;
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::network::p2p::payloads::witness::Witness;
use neo_core::persistence::providers::MemoryStoreProvider;
use neo_core::persistence::IStoreProvider;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::native::{GasToken, NativeContract, NeoToken};
use neo_core::smart_contract::notify_event_args::NotifyEventArgs;
use neo_core::smart_contract::TriggerType;
use neo_core::tokens_tracker::{
    find_prefix, Nep17Tracker, Nep17TransferKey, TokenTransfer, TokenTransferKeyView, Tracker,
};
use neo_core::{UInt160, WitnessScope};
use neo_vm::{StackItem, VMState};
use num_bigint::BigInt;
use std::sync::Arc;

#[tokio::test(flavor = "multi_thread")]
async fn nep17_tracker_matches_csharp_history_indexing() {
    let system = Arc::new(NeoSystem::new(ProtocolSettings::mainnet(), None, None).expect("system"));
    let provider = MemoryStoreProvider::new();
    let tracker_store = provider.get_store("nep17-history").expect("tracker store");
    let mut tracker =
        Nep17Tracker::new(Arc::clone(&tracker_store), 1000, true, Arc::clone(&system));

    let source = UInt160::parse("0x71e1dae538237e26e083a777cebafa0a2f06fd43").expect("source");
    let recipient =
        UInt160::parse("0x8cf36fbcb4775f7ca41cb1c49a4f43c774b97e99").expect("recipient");

    let mut tx = Transaction::new();
    tx.set_nonce(1);
    tx.set_script(vec![0x51]);
    tx.set_signers(vec![Signer::new(source, WitnessScope::CALLED_BY_ENTRY)]);
    tx.set_witnesses(vec![Witness::empty()]);
    let tx_hash = tx.hash();
    let tx_container: Arc<dyn neo_core::IVerifiable> = Arc::new(tx.clone());

    let block = Block::new(
        BlockHeader {
            index: 38_781,
            timestamp: 1_628_511_122_592,
            ..Default::default()
        },
        vec![tx.clone()],
    );

    let gas = GasToken::new().hash();
    let neo = NeoToken::new().hash();
    let executed = vec![
        ApplicationExecuted {
            transaction: None,
            trigger: TriggerType::OnPersist,
            vm_state: VMState::HALT,
            exception: None,
            gas_consumed: 0,
            stack: Vec::new(),
            notifications: vec![NotifyEventArgs::new_with_optional_container(
                None,
                gas,
                "Transfer".to_string(),
                vec![
                    StackItem::from_byte_string(source.to_bytes()),
                    StackItem::null(),
                    StackItem::from_int(6_229_065i64),
                ],
            )],
            logs: Vec::new(),
        },
        ApplicationExecuted {
            transaction: Some(tx.clone()),
            trigger: TriggerType::Application,
            vm_state: VMState::HALT,
            exception: None,
            gas_consumed: 0,
            stack: Vec::new(),
            notifications: vec![
                NotifyEventArgs::new(
                    Arc::clone(&tx_container),
                    neo,
                    "Transfer".to_string(),
                    vec![
                        StackItem::from_byte_string(source.to_bytes()),
                        StackItem::from_byte_string(recipient.to_bytes()),
                        StackItem::from_int(33i64),
                    ],
                ),
                NotifyEventArgs::new(
                    tx_container,
                    gas,
                    "Transfer".to_string(),
                    vec![
                        StackItem::null(),
                        StackItem::from_byte_string(source.to_bytes()),
                        StackItem::from_int(660i64),
                    ],
                ),
            ],
            logs: Vec::new(),
        },
    ];

    let store_cache = system.store_cache();
    let snapshot = store_cache.data_cache().clone();
    tracker.reset_batch();
    tracker.on_persist(system.as_ref(), &block, &snapshot, &executed);
    tracker.commit();

    let (_, sent_prefix, received_prefix) = Nep17Tracker::rpc_prefixes();
    let sent_key = [vec![sent_prefix], source.to_bytes().to_vec()].concat();
    let received_key = [vec![received_prefix], source.to_bytes().to_vec()].concat();

    let sent = find_prefix::<Nep17TransferKey, TokenTransfer>(tracker_store.as_ref(), &sent_key)
        .expect("sent transfers");
    let received =
        find_prefix::<Nep17TransferKey, TokenTransfer>(tracker_store.as_ref(), &received_key)
            .expect("received transfers");

    assert_eq!(sent.len(), 1);
    assert_eq!(received.len(), 1);

    let (sent_key, sent_value) = &sent[0];
    assert_eq!(sent_key.timestamp_ms(), block.header.timestamp);
    assert_eq!(sent_key.block_xfer_notification_index(), 0);
    assert_eq!(sent_value.tx_hash, tx_hash);
    assert_eq!(sent_value.amount, BigInt::from(33i64));
    assert_eq!(sent_value.user_script_hash, recipient);

    let (recv_key, recv_value) = &received[0];
    assert_eq!(recv_key.timestamp_ms(), block.header.timestamp);
    assert_eq!(recv_key.block_xfer_notification_index(), 1);
    assert_eq!(recv_value.tx_hash, tx_hash);
    assert_eq!(recv_value.amount, BigInt::from(660i64));
    assert_eq!(recv_value.user_script_hash, UInt160::zero());
}
