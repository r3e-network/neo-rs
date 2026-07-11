use crate::contract_management::{
    ContractManagement, DEFAULT_MINIMUM_DEPLOYMENT_FEE, DEFAULT_NEXT_AVAILABLE_ID,
};
use neo_config::{Hardfork, ProtocolSettings};
use neo_execution::native_contract::build_native_contract_state;
use neo_execution::{ApplicationEngine, ContractState};
use neo_manifest::{ContractManifest, ContractMethodDescriptor, NefFile};
use neo_payloads::VerifiableContainer;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::witness::Witness;
use neo_primitives::{CallFlags, ContractParameterType, TriggerType, UInt160, WitnessScope};
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::{OpCode, VmState};
use num_bigint::BigInt;
use std::sync::Arc;

/// The deploying transaction's sender (first signer).
pub(super) const SENDER: [u8; 20] = [0x07; 20];

/// Writes a serialized contract record under `Prefix_Contract ++ hash`.
pub(super) fn put_contract_record(cache: &DataCache, state: &ContractState) {
    cache.add(
        ContractManagement::contract_storage_key(&state.hash),
        StorageItem::from_bytes(
            ContractManagement::serialize_contract_record(state).expect("record bytes"),
        ),
    );
}

fn seed_contract_management_settings(cache: &DataCache) {
    cache.add(
        ContractManagement::minimum_deployment_fee_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            DEFAULT_MINIMUM_DEPLOYMENT_FEE,
        ))),
    );
    cache.add(
        ContractManagement::next_available_id_key(),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
            DEFAULT_NEXT_AVAILABLE_ID,
        ))),
    );
}

/// Snapshot seeded with the ContractManagement native record so
/// `System.Contract.Call` resolves the callee.
pub(super) fn seeded_snapshot() -> Arc<DataCache> {
    let cache = DataCache::new(false);
    seed_contract_management_settings(&cache);
    put_contract_record(
        &cache,
        &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
    );
    Arc::new(cache)
}

pub(super) fn faun_from_genesis_settings() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    settings.hardforks.insert(Hardfork::HfFaun, 0);
    settings
}

/// The smallest NEF that parses: a single RET at offset 0.
pub(super) fn minimal_nef() -> NefFile {
    NefFile::new("e2e-test".to_string(), vec![OpCode::RET.byte()])
}

/// A minimal deployable manifest: `main()` at offset 0.
pub(super) fn deployable_manifest(name: &str) -> ContractManifest {
    let mut manifest = ContractManifest::new(name.to_string());
    manifest.abi.methods.push(
        ContractMethodDescriptor::new(
            "main".to_string(),
            vec![],
            ContractParameterType::Void,
            0,
            true,
        )
        .expect("method descriptor"),
    );
    manifest
}

/// JSON payload for a manifest (what a deploying transaction carries).
pub(super) fn manifest_json(manifest: &ContractManifest) -> Vec<u8> {
    manifest
        .to_json()
        .expect("manifest json")
        .to_string()
        .into_bytes()
}

pub(super) fn engine_for(
    snapshot: Arc<DataCache>,
    settings: ProtocolSettings,
    sender: UInt160,
) -> ApplicationEngine<crate::StandardNativeProvider> {
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(sender, WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    let container = Arc::new(VerifiableContainer::from(tx));
    ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        snapshot,
        None,
        settings,
        1000_00000000, // covers the 10-GAS minimum deployment fee
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
    )
    .expect("engine builds")
}

/// Runs `System.Contract.Call(CM, "deploy", [nef, manifest(, data)])` and
/// returns the final VM state plus the engine (for fee / notification /
/// result-stack assertions).
pub(super) fn run_deploy(
    snapshot: &Arc<DataCache>,
    settings: ProtocolSettings,
    sender: UInt160,
    nef_bytes: &[u8],
    manifest_bytes: &[u8],
    data: Option<&[u8]>,
    flags: CallFlags,
) -> (VmState, ApplicationEngine<crate::StandardNativeProvider>) {
    let mut builder = ScriptBuilder::new();
    // Args are pushed deepest-first (argN-1 .. arg0) before PACK.
    let argc = if let Some(data) = data {
        builder.emit_push(data);
        3
    } else {
        2
    };
    builder.emit_push(manifest_bytes);
    builder.emit_push(nef_bytes);
    builder.emit_push_int(argc);
    builder.emit_pack();
    builder.emit_push_int(i64::from(flags.bits()));
    builder.emit_push("deploy".as_bytes());
    builder.emit_push(&ContractManagement::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");

    let mut engine = engine_for(Arc::clone(snapshot), settings, sender);
    engine
        .load_script(builder.to_array(), CallFlags::ALL, None)
        .expect("script loads");
    let state = engine.execute_allow_fault();
    (state, engine)
}

/// Builds the self-update entry script
/// `System.Contract.Call(CM, "update", [nef?, manifest?])`; `None` pushes
/// the C# `null` argument.
pub(super) fn update_script(
    nef_bytes: Option<&[u8]>,
    manifest_bytes: Option<&[u8]>,
    flags: CallFlags,
) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    // arg1 (manifest) deepest, then arg0 (nef) on top, then PACK 2.
    match manifest_bytes {
        Some(bytes) => {
            builder.emit_push(bytes);
        }
        None => {
            builder.emit_opcode(OpCode::PUSHNULL);
        }
    }
    match nef_bytes {
        Some(bytes) => {
            builder.emit_push(bytes);
        }
        None => {
            builder.emit_opcode(OpCode::PUSHNULL);
        }
    }
    builder.emit_push_int(2);
    builder.emit_pack();
    builder.emit_push_int(i64::from(flags.bits()));
    builder.emit_push("update".as_bytes());
    builder.emit_push(&ContractManagement::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");
    builder.to_array()
}

/// Builds the self-update entry script
/// `System.Contract.Call(CM, "update", [nef?, manifest?, data])`.
pub(super) fn update_script_with_data(
    nef_bytes: Option<&[u8]>,
    manifest_bytes: Option<&[u8]>,
    data: &[u8],
    flags: CallFlags,
) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_push(data);
    match manifest_bytes {
        Some(bytes) => {
            builder.emit_push(bytes);
        }
        None => {
            builder.emit_opcode(OpCode::PUSHNULL);
        }
    }
    match nef_bytes {
        Some(bytes) => {
            builder.emit_push(bytes);
        }
        None => {
            builder.emit_opcode(OpCode::PUSHNULL);
        }
    }
    builder.emit_push_int(3);
    builder.emit_pack();
    builder.emit_push_int(i64::from(flags.bits()));
    builder.emit_push("update".as_bytes());
    builder.emit_push(&ContractManagement::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");
    builder.to_array()
}

/// Runs a self-update entry script whose pinned hash is `self_hash`.
pub(super) fn run_update(
    snapshot: &Arc<DataCache>,
    script: Vec<u8>,
    self_hash: UInt160,
) -> (VmState, ApplicationEngine<crate::StandardNativeProvider>) {
    let sender = UInt160::from_bytes(&SENDER).unwrap();
    let mut engine = engine_for(Arc::clone(snapshot), ProtocolSettings::default(), sender);
    engine
        .load_script(script, CallFlags::ALL, Some(self_hash))
        .expect("script loads");
    let state = engine.execute_allow_fault();
    (state, engine)
}
