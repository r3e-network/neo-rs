use std::cmp::Ordering;
use std::sync::Arc;
use std::vec::Vec;

use crate::crypto::keys::{self, PublicKey};
use crate::smartcontract;
use crate::wallet::{self, Account};
use crate::require;

#[test]
fn test_single_signer() {
    let a = wallet::Account::new().unwrap();
    let s = NewSingleSigner::new(a);
    assert_eq!(s.script_hash(), s.account().contract().script_hash());
}

#[test]
fn test_multi_signer() {
    const SIZE: usize = 4;

    let mut pubs: Vec<PublicKey> = Vec::with_capacity(SIZE);
    let mut accs: Vec<Account> = Vec::with_capacity(SIZE);
    for _ in 0..SIZE {
        let a = wallet::Account::new().unwrap();
        accs.push(a.clone());
        pubs.push(a.public_key().clone());
    }

    pubs.sort_by(|a, b| a.cmp(b));
    let m = smartcontract::get_default_honest_node_count(SIZE);
    for acc in &mut accs {
        acc.convert_multisig(m, &pubs).unwrap();
    }

    let s = NewMultiSigner::new(accs.clone());
    for (i, pub) in pubs.iter().enumerate() {
        for acc in &accs {
            if acc.public_key() == pub {
                assert_eq!(pub, s.single(i).account().public_key());
            }
        }
    }
}
