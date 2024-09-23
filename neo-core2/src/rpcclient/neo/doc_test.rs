use std::cmp;
use std::context::Context;
use std::sync::Arc;
use std::sync::Mutex;
use std::collections::HashMap;
use bigdecimal::BigDecimal;
use crate::core::transaction::{self, Transaction};
use crate::encoding::address;
use crate::rpcclient::{self, invoker, neo, unwrap};
use crate::util::{self, Uint160, Uint256};
use crate::vm::stackitem::Item;
use crate::neorpc::result::{self, Invoke, Iterator};
use crate::smartcontract::{self, Parameter};
use crate::vm::vmstate::VmState;
use crate::smartcontract::manifest::Manifest;
use crate::smartcontract::nef::NefFile;
use crate::vm::stackitem::StackItem;
use crate::vm::stackitem::StackItem::Null;
use crate::vm::stackitem::StackItem::Struct;
use crate::vm::stackitem::StackItem::Interop;
use crate::vm::stackitem::StackItem::ByteArray;
use crate::vm::stackitem::StackItem::Integer;
use crate::vm::stackitem::StackItem::Array;
use crate::vm::stackitem::StackItem::Map;
use crate::vm::stackitem::StackItem::Boolean;
use crate::vm::stackitem::StackItem::Buffer;
use crate::vm::stackitem::StackItem::Pointer;
use crate::vm::stackitem::StackItem::Any;
use crate::vm::stackitem::StackItem::Struct;
use crate::vm::stackitem::StackItem::Interop;
use crate::vm::stackitem::StackItem::ByteArray;
use crate::vm::stackitem::StackItem::Integer;
use crate::vm::stackitem::StackItem::Array;
use crate::vm::stackitem::StackItem::Map;
use crate::vm::stackitem::StackItem::Boolean;

fn example_contract_reader() {
    // No error checking done at all, intentionally.
    let c = rpcclient::new(Context::background(), "url", rpcclient::Options::default()).unwrap();

    // Safe methods are reachable with just an invoker, no need for an account there.
    let inv = invoker::new(c, None);

    // Create a reader interface.
    let neo_token = neo::new_reader(inv);

    // Account hash we're interested in.
    let acc_hash = address::string_to_uint160("NdypBhqkz2CMMnwxBgvoC9X2XjKF5axgKo").unwrap();

    // Get the account balance.
    let balance = neo_token.balance_of(acc_hash).unwrap();
    let _ = balance;

    // Get the extended NEO-specific balance data.
    let b_neo = neo_token.get_account_state(acc_hash).unwrap();

    // Account can have no associated vote.
    if b_neo.vote_to.is_none() {
        return;
    }
    // Committee keys.
    let comm = neo_token.get_committee().unwrap();

    // Check if the vote is made for a committee member.
    let mut voted_for_committee_member = false;
    for i in 0..comm.len() {
        if b_neo.vote_to.as_ref().unwrap().equal(&comm[i]) {
            voted_for_committee_member = true;
            break;
        }
    }
    let _ = voted_for_committee_member;
}

fn example_contract() {
    // No error checking done at all, intentionally.
    let w = wallet::new_wallet_from_file("somewhere").unwrap();
    defer!(w.close());

    let c = rpcclient::new(Context::background(), "url", rpcclient::Options::default()).unwrap();

    // Create a simple CalledByEntry-scoped actor (assuming there is an account
    // inside the wallet).
    let a = actor::new_simple(c, w.accounts[0].clone()).unwrap();

    // Create a complete contract representation.
    let neo_token = neo::new(a);

    let tgt_acc = address::string_to_uint160("NdypBhqkz2CMMnwxBgvoC9X2XjKF5axgKo").unwrap();

    // Send a transaction that transfers one token to another account.
    let (txid, vub) = neo_token.transfer(a.sender(), tgt_acc, BigDecimal::from(1), None).unwrap();
    let _ = txid;
    let _ = vub;

    // Get a list of candidates (it's limited, but should be sufficient in most cases).
    let cands = neo_token.get_candidates().unwrap();

    // Sort by votes.
    cands.sort_by(|a, b| cmp::compare(a.votes, b.votes));

    // Get the extended NEO-specific balance data.
    let b_neo = neo_token.get_account_state(a.sender()).unwrap();

    // If not yet voted, or voted for suboptimal candidate (we want the one with the least votes),
    // send a new voting transaction
    if b_neo.vote_to.is_none() || !b_neo.vote_to.as_ref().unwrap().equal(&cands[0].public_key) {
        let (txid, vub) = neo_token.vote(a.sender(), &cands[0].public_key).unwrap();
        let _ = txid;
        let _ = vub;
    }
}
