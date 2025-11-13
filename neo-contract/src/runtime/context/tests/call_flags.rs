use neo_base::{hash::Hash160, Bytes};
use neo_store::{ColumnId, MemoryStore};
use neo_vm::{VmError, VmValue};

use crate::{
    manifest::{
        ContractAbi, ContractFeatures, ContractManifest, ContractMethod, ContractParameter,
        ContractPermission, ParameterKind, WildcardContainer,
    },
    nef::{CallFlags, NefFile},
    runtime::{
        context::tests::helpers::new_context,
        contract_store::{contract_column, put_contract_state},
        ExecutionContext,
    },
    state::ContractState,
};

fn ctx_with_flags(store: &mut MemoryStore, flags: CallFlags) -> ExecutionContext<'_> {
    let mut ctx = new_context(store);
    ctx.set_call_flags(flags);
    ctx
}

fn fixed_hash(byte: u8) -> Hash160 {
    Hash160::from_slice(&[byte; 20]).expect("hash")
}

fn sample_manifest(method: &str) -> ContractManifest {
    ContractManifest {
        name: "Test".into(),
        groups: vec![],
        features: ContractFeatures::default(),
        supported_standards: vec![],
        abi: ContractAbi {
            methods: vec![ContractMethod {
                name: method.into(),
                parameters: vec![],
                return_type: ParameterKind::Void,
                offset: 0,
                safe: false,
            }],
            events: vec![],
        },
        permissions: vec![ContractPermission::allow_all()],
        trusts: WildcardContainer::wildcard(),
        extra: Default::default(),
    }
}

fn sample_contract_state(hash: Hash160, method: &str) -> ContractState {
    let nef = NefFile::new("unit-test", "", vec![], vec![0x6A]).expect("nef");
    ContractState::new(1, hash, nef, sample_manifest(method))
}

fn insert_contract(store: &MemoryStore, state: &ContractState) {
    store.create_column(contract_column());
    put_contract_state(store, state).expect("store contract");
}

#[test]
fn storage_read_requires_read_states() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));
    store
        .put(
            ColumnId::new("contract"),
            b"key".to_vec(),
            b"value".to_vec(),
        )
        .unwrap();
    let ctx = ctx_with_flags(&mut store, CallFlags::WRITE_STATES);
    let err = ctx.load(ColumnId::new("contract"), b"key").unwrap_err();
    assert!(matches!(
        err,
        crate::error::ContractError::MissingCallFlags(flag) if flag == CallFlags::READ_STATES
    ));
}

#[test]
fn storage_write_requires_write_states() {
    let mut store = MemoryStore::new();
    store.create_column(ColumnId::new("contract"));
    let mut ctx = ctx_with_flags(&mut store, CallFlags::READ_STATES);
    let err = ctx
        .put(
            ColumnId::new("contract"),
            b"key".to_vec(),
            b"value".to_vec(),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        crate::error::ContractError::MissingCallFlags(flag) if flag == CallFlags::WRITE_STATES
    ));
}

#[test]
fn contract_call_rejects_invalid_flag_bits() {
    let mut store = MemoryStore::new();
    let mut ctx = ctx_with_flags(&mut store, CallFlags::ALL);
    let err = ctx
        .handle_contract_call(&fixed_hash(0x01), "foo", 0x80, Vec::new())
        .unwrap_err();
    assert_eq!(err, VmError::InvalidType);
}

#[test]
fn contract_call_requires_subset_of_current_flags() {
    let mut store = MemoryStore::new();
    let mut ctx = ctx_with_flags(&mut store, CallFlags::READ_STATES);
    let err = ctx
        .handle_contract_call(
            &fixed_hash(0x02),
            "foo",
            CallFlags::WRITE_STATES.bits(),
            Vec::new(),
        )
        .unwrap_err();
    assert!(matches!(err, VmError::NativeFailure(msg) if msg == "insufficient call flags"));
}

#[test]
fn contract_call_reports_not_supported_when_valid() {
    let mut store = MemoryStore::new();
    let hash = fixed_hash(0x03);
    insert_contract(&store, &sample_contract_state(hash, "foo"));
    let mut ctx = ctx_with_flags(&mut store, CallFlags::ALL);
    let err = ctx
        .handle_contract_call(
            &hash,
            "foo",
            CallFlags::READ_STATES.bits(),
            vec![VmValue::Int(1)],
        )
        .unwrap_err();
    assert!(
        matches!(err, VmError::NativeFailure(msg) if msg == "contract calls are not supported yet")
    );
}

#[test]
fn contract_call_requires_existing_contract() {
    let mut store = MemoryStore::new();
    let mut ctx = ctx_with_flags(&mut store, CallFlags::ALL);
    let err = ctx
        .handle_contract_call(&fixed_hash(0x04), "foo", CallFlags::READ_STATES.bits(), Vec::new())
        .unwrap_err();
    assert!(matches!(err, VmError::NativeFailure(msg) if msg == "contract not found"));
}

#[test]
fn contract_call_requires_method() {
    let mut store = MemoryStore::new();
    let hash = fixed_hash(0x05);
    insert_contract(&store, &sample_contract_state(hash, "bar"));
    let mut ctx = ctx_with_flags(&mut store, CallFlags::ALL);
    let err = ctx
        .handle_contract_call(&hash, "foo", CallFlags::READ_STATES.bits(), Vec::new())
        .unwrap_err();
    assert!(matches!(err, VmError::NativeFailure(msg) if msg == "method not found"));
}
