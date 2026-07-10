use std::sync::Arc;

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcHandler;
use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
use neo_primitives::{TriggerType, UInt160, WitnessScope};
use neo_storage::persistence::{Store, StoreSnapshot, WriteStore};
use neo_vm::StackItem;
use neo_vm_rs::VmState as VMState;

pub(super) fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .expect("handler must be registered")
}

pub(super) fn assert_invalid_params(err: RpcException, expected_message: &str) {
    assert_eq!(
        err.code(),
        crate::server::rpc_error::RpcError::invalid_params().code()
    );
    assert!(err.to_string().contains(expected_message), "{err}");
}

pub(super) fn account(seed: u8) -> UInt160 {
    UInt160::from_bytes(&[seed; UInt160::LENGTH]).expect("valid account")
}

pub(super) fn transaction(nonce: u32, accounts: &[UInt160]) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(vec![u8::try_from(nonce % 251).expect("bounded")]);
    tx.set_signers(
        accounts
            .iter()
            .copied()
            .map(|account| Signer::new(account, WitnessScope::CALLED_BY_ENTRY))
            .collect(),
    );
    tx
}

pub(super) fn block(height: u32, transactions: Vec<Transaction>) -> Block {
    let mut header = Header::new();
    header.set_index(height);
    header.set_timestamp(1_700_000_000_000 + u64::from(height));
    header.set_nonce(u64::from(height));
    let mut block = Block::from_parts(header, transactions);
    block.try_rebuild_merkle_root().expect("merkle root");
    block
}

pub(super) fn execution(
    tx: Transaction,
    contract: UInt160,
    event_name: &str,
    state: Vec<StackItem>,
) -> ApplicationExecuted {
    let mut execution = ApplicationExecuted::new(
        Some(tx),
        TriggerType::APPLICATION,
        VMState::HALT,
        None,
        0,
        Vec::new(),
    );
    execution
        .notifications
        .push(NotifyEventArgs::new_with_optional_container(
            None,
            contract,
            event_name.to_string(),
            state,
        ));
    execution
}

pub(super) fn transfer_state(from: UInt160, to: UInt160, amount: i64) -> Vec<StackItem> {
    vec![
        StackItem::from_byte_string(from.to_bytes()),
        StackItem::from_byte_string(to.to_bytes()),
        StackItem::from_int(amount),
    ]
}

pub(super) fn corrupt_block_by_height_record<S>(store: &Arc<S>, height: u32)
where
    S: Store,
{
    let mut key = b"neo-indexer:v3:block-by-height:".to_vec();
    key.extend_from_slice(&height.to_be_bytes());
    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).expect("unique store snapshot");
    snapshot
        .put(key, b"not-json".to_vec())
        .expect("put corrupt record");
    snapshot.try_commit().expect("commit corrupt record");
}
