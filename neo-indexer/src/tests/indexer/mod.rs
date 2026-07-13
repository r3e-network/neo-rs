//! # neo-indexer::tests::indexer
//!
//! Test module grouping Indexer workers and projection logic for chain-derived
//! data. coverage for neo-indexer.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-indexer; it may assemble fixtures
//! but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `blocks`: block projection coverage.
//! - `notifications`: notification projection and query logic.
//! - `snapshots`: snapshot projection coverage.

use super::*;
use crate::{INDEXER_SNAPSHOT_VERSION, IndexerSnapshot};
use neo_payloads::{ApplicationExecuted, Block, Header, NotifyEventArgs, Signer, Transaction};
use neo_primitives::{TriggerType, WitnessScope};
use neo_vm::VmState as VMState;
use neo_vm::{StackItem, StackItemRpcJson};

fn account(seed: u8) -> UInt160 {
    UInt160::from_bytes(&[seed; UInt160::LENGTH]).expect("valid account")
}

fn hash256(seed: u8) -> UInt256 {
    UInt256::from_bytes(&[seed; UInt256::LENGTH]).expect("valid hash")
}

fn transaction(nonce: u32, accounts: &[UInt160]) -> Transaction {
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

fn block(height: u32, nonce: u64, transactions: Vec<Transaction>) -> Block {
    let mut header = Header::new();
    header.set_index(height);
    header.set_timestamp(1_700_000_000_000 + u64::from(height));
    header.set_nonce(nonce);
    let mut block = Block::from_parts(header, transactions);
    block.try_rebuild_merkle_root().expect("merkle root");
    block
}

fn execution(
    transaction: Option<Transaction>,
    contract: UInt160,
    event_name: &str,
) -> ApplicationExecuted {
    execution_with_state(transaction, contract, event_name, Vec::new())
}

fn execution_with_state(
    transaction: Option<Transaction>,
    contract: UInt160,
    event_name: &str,
    state: Vec<StackItem>,
) -> ApplicationExecuted {
    let trigger = if transaction.is_some() {
        TriggerType::APPLICATION
    } else {
        TriggerType::ON_PERSIST
    };
    let mut execution =
        ApplicationExecuted::new(transaction, trigger, VMState::HALT, None, 0, Vec::new());
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

fn transfer_state(from: Option<UInt160>, to: Option<UInt160>, amount: i64) -> Vec<StackItem> {
    vec![
        from.map_or(StackItem::Null, |account| {
            StackItem::from_byte_string(account.to_bytes())
        }),
        to.map_or(StackItem::Null, |account| {
            StackItem::from_byte_string(account.to_bytes())
        }),
        StackItem::from_int(amount),
    ]
}

fn state_json(state: &[StackItem]) -> Vec<serde_json::Value> {
    state
        .iter()
        .map(|item| StackItemRpcJson::stack_item_rpc_json(item, None).expect("stack JSON"))
        .collect()
}

#[path = "blocks.rs"]
mod blocks;
#[path = "notifications.rs"]
mod notifications;
#[path = "snapshots.rs"]
mod snapshots;
