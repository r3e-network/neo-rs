use std::collections::HashMap;
use std::convert::TryInto;

use neo_core::{
    crypto::{hash, keys},
    interop::{self, Context},
    storage,
    transaction::{self, Signer, Witness},
    util,
    vm::{self, opcode, stackitem},
};
use neo_core::config::netmode;
use neo_core::core::dao;
use neo_core::core::fee;
use neo_core::core::native;
use neo_core::smartcontract::trigger;
use neo_core::vm::stackitem::StackItem;
use neo_core::vm::VM;
use neo_core::internal::fakechain;
use assert2::{assert, check};
use anyhow::Result;

fn init_check_multisig(msg_hash: util::Uint256, n: usize) -> Result<(Vec<StackItem>, Vec<StackItem>, HashMap<String, keys::PublicKey>)> {
    let mut key_map = HashMap::new();
    let mut pkeys = Vec::with_capacity(n);
    let mut pubs = Vec::with_capacity(n);

    for _ in 0..n {
        let pkey = keys::PrivateKey::new()?;
        let pk = pkey.public_key();
        let data = pk.bytes();
        pubs.push(StackItem::ByteArray(data.clone()));
        key_map.insert(data, pk);
        pkeys.push(pkey);
    }

    let sigs = pkeys.iter().map(|pkey| {
        let sig = pkey.sign_hash(&msg_hash);
        StackItem::ByteArray(sig)
    }).collect();

    Ok((pubs, sigs, key_map))
}

fn sub_slice(arr: &[StackItem], indices: Option<&[usize]>) -> Vec<StackItem> {
    match indices {
        Some(indices) => indices.iter().map(|&i| arr[i].clone()).collect(),
        None => arr.to_vec(),
    }
}

fn init_check_multisig_vm_no_args(container: &transaction::Transaction) -> VM {
    let mut buf = vec![0; 5];
    buf[0] = opcode::SYSCALL as u8;
    buf[1..5].copy_from_slice(&neo_crypto_check_multisig_id().to_le_bytes());

    let ic = Context::new(
        trigger::Verification,
        fakechain::FakeChain::new(),
        dao::Simple::new(storage::MemoryStore::new(), false),
        interop::DEFAULT_BASE_EXEC_FEE,
        native::DEFAULT_STORAGE_PRICE,
        None,
        None,
        None,
        None,
        Some(container.clone()),
        None,
    );
    ic.container = Some(container.clone());
    ic.functions = interops();
    let mut v = ic.spawn_vm();
    v.load_script(&buf);
    v
}

fn init_check_multisig_vm(t: &mut testing::T, n: usize, ik: Option<&[usize]>, is: Option<&[usize]>) -> VM {
    let tx = transaction::Transaction::new(b"NEO - An Open Network For Smart Economy".to_vec(), 10);
    tx.signers = vec![Signer { account: util::Uint160::from([1, 2, 3]) }];
    tx.scripts = vec![Witness::default()];

    let mut v = init_check_multisig_vm_no_args(&tx);

    let (mut pubs, mut sigs, _) = init_check_multisig(hash::net_sha256(netmode::UNIT_TEST_NET, &tx), n).unwrap();
    pubs = sub_slice(&pubs, ik);
    sigs = sub_slice(&sigs, is);

    v.estack().push_val(sigs);
    v.estack().push_val(pubs);

    v
}

fn test_check_multisig_good(t: &mut testing::T, n: usize, is: &[usize]) {
    let mut v = init_check_multisig_vm(t, n, None, Some(is));

    assert!(v.run().is_ok());
    assert!(v.estack().len() == 1);
    assert!(v.estack().pop().unwrap().as_bool().unwrap());
}

#[test]
fn test_ecdsa_secp256r1_check_multisig_good() {
    test_curve_check_multisig_good();
}

fn test_curve_check_multisig_good() {
    test_check_multisig_good(&mut testing::T::new(), 3, &[1]);
    test_check_multisig_good(&mut testing::T::new(), 2, &[0, 1]);
    test_check_multisig_good(&mut testing::T::new(), 3, &[0, 1, 2]);
    test_check_multisig_good(&mut testing::T::new(), 3, &[0, 2]);
    test_check_multisig_good(&mut testing::T::new(), 4, &[0, 2]);
    test_check_multisig_good(&mut testing::T::new(), 10, &[2, 3, 4, 5, 6, 8, 9]);
    test_check_multisig_good(&mut testing::T::new(), 12, &[0, 1, 4, 5, 6, 7, 8, 9]);
}

fn test_check_multisig_bad(t: &mut testing::T, is_err: bool, n: usize, ik: Option<&[usize]>, is: Option<&[usize]>) {
    let mut v = init_check_multisig_vm(t, n, ik, is);

    if is_err {
        assert!(v.run().is_err());
        return;
    }
    assert!(v.run().is_ok());
    assert!(v.estack().len() == 1);
    assert!(!v.estack().pop().unwrap().as_bool().unwrap());
}

#[test]
fn test_ecdsa_secp256r1_check_multisig_bad() {
    test_curve_check_multisig_bad();
}

fn test_curve_check_multisig_bad() {
    test_check_multisig_bad(&mut testing::T::new(), false, 2, Some(&[0]), Some(&[1]));
    test_check_multisig_bad(&mut testing::T::new(), false, 3, Some(&[0, 2]), Some(&[2, 0]));
    test_check_multisig_bad(&mut testing::T::new(), false, 3, None, Some(&[0, 0]));
    test_check_multisig_bad(&mut testing::T::new(), true, 2, Some(&[0]), Some(&[0, 1]));
    test_check_multisig_bad(&mut testing::T::new(), true, 1, Some(&[0]), Some(&[0]));

    let msg = b"NEO - An Open Network For Smart Economy".to_vec();
    let (pubs, sigs, _) = init_check_multisig(hash::sha256(&msg), 1).unwrap();
    let arr = stackitem::Array::new(vec![stackitem::Array::new(vec![])]);
    let tx = transaction::Transaction::new(b"NEO - An Open Network For Smart Economy".to_vec(), 10);
    tx.signers = vec![Signer { account: util::Uint160::from([1, 2, 3]) }];
    tx.scripts = vec![Witness::default()];

    let mut v = init_check_multisig_vm_no_args(&tx);
    v.estack().push_val(sigs);
    v.estack().push_val(arr);
    assert!(v.run().is_err());

    let mut v = init_check_multisig_vm_no_args(&tx);
    v.estack().push_val(arr);
    v.estack().push_val(pubs);
    assert!(v.run().is_err());
}

#[test]
fn test_check_sig() {
    let priv_key = keys::PrivateKey::new().unwrap();

    let verify_func = ecdsa_secp256r1_check_sig;
    let d = dao::Simple::new(storage::MemoryStore::new(), false);
    let mut ic = Context::new(
        trigger::Verification,
        fakechain::FakeChain::new(),
        d,
        interop::DEFAULT_BASE_EXEC_FEE,
        native::DEFAULT_STORAGE_PRICE,
        None,
        None,
        None,
        None,
        None,
        None,
    );
    ic.network = netmode::UNIT_TEST_NET;

    let run_case = |t: &mut testing::T, is_err: bool, result: bool, args: Vec<StackItem>| {
        ic.spawn_vm();
        for arg in args {
            ic.vm.estack().push_val(arg);
        }

        let err = std::panic::catch_unwind(|| verify_func(&mut ic)).err();

        if is_err {
            assert!(err.is_some());
            return;
        }
        assert!(err.is_none());
        assert!(ic.vm.estack().len() == 1);
        assert!(ic.vm.estack().pop().unwrap().as_bool().unwrap() == result);
    };

    let tx = transaction::Transaction::new(vec![0, 1, 2], 1);
    ic.container = Some(tx.clone());

    let sign = priv_key.sign_hashable(netmode::UNIT_TEST_NET, &tx);
    run_case(&mut testing::T::new(), false, true, vec![StackItem::ByteArray(sign.clone()), StackItem::ByteArray(priv_key.public_key().bytes())]);

    run_case(&mut testing::T::new(), true, false, vec![]);
    run_case(&mut testing::T::new(), true, false, vec![StackItem::ByteArray(sign.clone())]);

    let mut invalid_sign = sign.clone();
    invalid_sign[0] ^= 0xFF;
    run_case(&mut testing::T::new(), false, false, vec![StackItem::ByteArray(invalid_sign), StackItem::ByteArray(priv_key.public_key().bytes())]);

    let mut invalid_pub = priv_key.public_key().bytes();
    invalid_pub[0] = 0xFF;
    run_case(&mut testing::T::new(), true, false, vec![StackItem::ByteArray(sign), StackItem::ByteArray(invalid_pub)]);
}
