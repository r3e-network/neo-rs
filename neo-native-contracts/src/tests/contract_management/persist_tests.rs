use super::*;
use std::collections::HashMap;
use std::sync::Arc;

use neo_config::{Hardfork, ProtocolSettings};
use neo_payloads::{Block, Header};
use neo_primitives::TriggerType;

fn settings_with(hardforks: &[(Hardfork, u32)]) -> ProtocolSettings {
    let schedule = hardforks.iter().copied().fold(
        neo_config::HardforkSchedule::new(),
        |schedule, (hardfork, height)| schedule.with_activation(hardfork, height),
    );
    ProtocolSettings {
        hardforks: schedule,
        ..ProtocolSettings::default()
    }
}

fn on_persist_engine(
    snapshot: &Arc<DataCache>,
    settings: &ProtocolSettings,
    index: u32,
    timestamp: u64,
) -> ApplicationEngine<crate::StandardNativeProvider> {
    let mut header = Header::new();
    header.set_index(index);
    header.set_timestamp(timestamp);
    let block = Block::from_parts(header, Vec::new());
    ApplicationEngine::new_with_native_contract_provider(
        TriggerType::OnPersist,
        None,
        Arc::clone(snapshot),
        Some(block),
        settings.clone(),
        0,
        neo_execution::NoDiagnostic,
        std::sync::Arc::new(crate::StandardNativeProvider::new()),
    )
    .expect("engine builds")
}

/// C# `ContractManagement.InitializeAsync` (ContractManagement.cs:53-61):
/// genesis seeds MinimumDeploymentFee = 10 GAS and NextAvailableId = 1.
#[test]
fn initialize_seeds_deployment_fee_and_next_id() {
    let settings = settings_with(&[]);
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
    NativeContract::initialize(&ContractManagement::new(), &mut engine).expect("initialize");

    assert_eq!(
        storage_key_int(&snapshot, ContractManagement::minimum_deployment_fee_key()),
        Some(BigInt::from(10_00000000i64))
    );
    assert_eq!(
        storage_key_int(&snapshot, ContractManagement::next_available_id_key()),
        Some(BigInt::from(1))
    );
    // The counter then hands out 1, 2, ... (C# GetNextAvailableId).
    assert_eq!(
        ContractManagement::new()
            .get_next_available_id(&snapshot)
            .unwrap(),
        1
    );
    assert_eq!(
        ContractManagement::new()
            .get_next_available_id(&snapshot)
            .unwrap(),
        2
    );
}

/// C# `ContractManagement.OnPersistAsync` at genesis: every genesis-active
/// native gets a `Prefix_Contract` record (UpdateCounter 0), a
/// `Prefix_ContractHash` id index entry, and a `Deploy` notification, in
/// the canonical contract order. Natives activating at an unscheduled
/// hardfork (Notary/Treasury here) are not deployed (C# IsInitializeBlock
/// skips unconfigured hardforks).
#[test]
fn on_persist_writes_genesis_records_and_deploy_notifications() {
    let settings = settings_with(&[]);
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
    NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("on_persist");

    let genesis_native_names = [
        "ContractManagement",
        "StdLib",
        "CryptoLib",
        "LedgerContract",
        "NeoToken",
        "GasToken",
        "PolicyContract",
        "RoleManagement",
        "OracleContract",
    ];
    // C# interleaves native initialization with deployment: the
    // genesis-active NEO/GAS initializers emit Transfer before their
    // corresponding Deploy notifications.
    let notifications = engine.notifications();
    assert_eq!(notifications.len(), genesis_native_names.len() + 2);
    assert_eq!(notifications[0].event_name, "Deploy");
    assert_eq!(
        notifications[0].state()[0].as_bytes().unwrap(),
        crate::ContractManagement::script_hash().to_bytes()
    );
    assert_eq!(notifications[1].event_name, "Deploy");
    assert_eq!(
        notifications[1].state()[0].as_bytes().unwrap(),
        crate::StdLib::script_hash().to_bytes()
    );
    assert_eq!(notifications[2].event_name, "Deploy");
    assert_eq!(
        notifications[2].state()[0].as_bytes().unwrap(),
        crate::CryptoLib::script_hash().to_bytes()
    );
    assert_eq!(notifications[3].event_name, "Deploy");
    assert_eq!(
        notifications[3].state()[0].as_bytes().unwrap(),
        crate::LedgerContract::script_hash().to_bytes()
    );
    assert_eq!(notifications[4].event_name, "Transfer");
    assert_eq!(notifications[4].script_hash, crate::NeoToken::script_hash());
    assert_eq!(notifications[5].event_name, "Deploy");
    assert_eq!(
        notifications[5].state()[0].as_bytes().unwrap(),
        crate::NeoToken::script_hash().to_bytes()
    );
    assert_eq!(notifications[6].event_name, "Transfer");
    assert_eq!(notifications[6].script_hash, crate::GasToken::script_hash());
    assert_eq!(notifications[7].event_name, "Deploy");
    assert_eq!(
        notifications[7].state()[0].as_bytes().unwrap(),
        crate::GasToken::script_hash().to_bytes()
    );
    let deploy_notifications = notifications
        .iter()
        .filter(|notification| notification.event_name == "Deploy");
    for (notification, contract) in deploy_notifications.zip(NATIVE_CONTRACTS.iter()) {
        assert_eq!(notification.event_name, "Deploy");
        assert_eq!(notification.script_hash, ContractManagement::script_hash());
        assert_eq!(
            notification.state()[0].as_bytes().unwrap(),
            contract.hash().to_bytes(),
            "Deploy order follows the canonical contract order"
        );
    }

    for (contract, name) in NATIVE_CONTRACTS.iter().zip(genesis_native_names.iter()) {
        assert_eq!(contract.name(), *name, "canonical registration order");
        let state = ContractManagement::get_contract_from_snapshot(&snapshot, &contract.hash())
            .unwrap()
            .unwrap_or_else(|| panic!("{name} record missing"));
        assert_eq!(state.id, contract.id());
        assert_eq!(state.hash, contract.hash());
        assert_eq!(state.update_counter, 0);
        assert_eq!(state.manifest.name, *name);
        // The id -> hash index dereferences back to the same record.
        let by_id = ContractManagement::get_contract_by_id_from_snapshot(&snapshot, contract.id())
            .unwrap()
            .unwrap_or_else(|| panic!("{name} id index missing"));
        assert_eq!(by_id.hash, contract.hash());
    }

    // Unscheduled ActiveIn hardforks: no record, no notification.
    assert!(
        ContractManagement::get_contract_from_snapshot(&snapshot, &crate::Notary::script_hash())
            .unwrap()
            .is_none()
    );
    assert!(
        ContractManagement::get_contract_from_snapshot(&snapshot, &crate::Treasury::script_hash())
            .unwrap()
            .is_none()
    );

    // A later non-hardfork block is a complete no-op.
    let mut engine = on_persist_engine(&snapshot, &settings, 1, 1000);
    NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("block 1");
    assert!(engine.notifications().is_empty());
}

/// The HF_Echidna activation block (ContractManagement.cs:93-115): natives
/// whose used hardforks include Echidna get their stored record refreshed
/// (UpdateCounter++ + the height-composed NEF/manifest) and an `Update`
/// notification; Notary (ActiveIn = Echidna) is deployed fresh; Policy's
/// Echidna re-initialization (PolicyContract.cs:144-152) seeds the
/// NotaryAssisted attribute fee and migrates the block-time settings.
#[test]
fn echidna_block_refreshes_manifests_and_runs_policy_reinitialization() {
    let settings = settings_with(&[(Hardfork::HfEchidna, 100)]);
    let snapshot = Arc::new(DataCache::new(false));
    // Genesis deployment pass.
    let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
    NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("genesis");

    // Pre-Echidna NEO manifest: NEP-17 only, no onNEP17Payment.
    let neo_hash = crate::NeoToken::script_hash();
    let pre = ContractManagement::get_contract_from_snapshot(&snapshot, &neo_hash)
        .unwrap()
        .unwrap();
    assert_eq!(pre.manifest.supported_standards, ["NEP-17"]);
    assert!(!ContractManagement::abi_has_method(
        &pre.manifest,
        "onNEP17Payment",
        3
    ));

    // The Echidna activation block.
    let mut engine = on_persist_engine(&snapshot, &settings, 100, 100_000);
    NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("echidna block");

    // NEO: refreshed in place — UpdateCounter 1, NEP-27 joins, the Echidna
    // ABI method appears, id/hash unchanged.
    let post = ContractManagement::get_contract_from_snapshot(&snapshot, &neo_hash)
        .unwrap()
        .unwrap();
    assert_eq!(post.update_counter, 1);
    assert_eq!(post.id, crate::NeoToken::ID);
    assert_eq!(post.manifest.supported_standards, ["NEP-17", "NEP-27"]);
    assert!(ContractManagement::abi_has_method(
        &post.manifest,
        "onNEP17Payment",
        3
    ));

    // Notary: deployed fresh at its activation block.
    let notary =
        ContractManagement::get_contract_from_snapshot(&snapshot, &crate::Notary::script_hash())
            .unwrap()
            .expect("Notary deploys at Echidna");
    assert_eq!(notary.update_counter, 0);

    // GAS carries no Echidna-gated metadata: untouched, no notification.
    let gas =
        ContractManagement::get_contract_from_snapshot(&snapshot, &crate::GasToken::script_hash())
            .unwrap()
            .unwrap();
    assert_eq!(gas.update_counter, 0);
    let gas_hash_bytes = crate::GasToken::script_hash().to_bytes();
    assert!(
        engine
            .notifications()
            .iter()
            .all(|n| n.state()[0].as_bytes().unwrap() != gas_hash_bytes)
    );

    // Notification kinds: Update for refreshed natives, Deploy for Notary.
    let kinds: HashMap<Vec<u8>, String> = engine
        .notifications()
        .iter()
        .map(|n| {
            (
                n.state()[0].as_bytes().unwrap().to_vec(),
                n.event_name.clone(),
            )
        })
        .collect();
    assert_eq!(
        kinds.get(&neo_hash.to_bytes().to_vec()),
        Some(&"Update".to_string())
    );
    assert_eq!(
        kinds.get(&crate::Notary::script_hash().to_bytes().to_vec()),
        Some(&"Deploy".to_string())
    );

    // Policy Echidna re-initialization (PolicyContract.cs:144-152).
    assert_eq!(
        snapshot
            .get(&crate::PolicyContract::attribute_fee_key(
                neo_primitives::TransactionAttributeType::NotaryAssisted.to_byte(),
            ))
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes())),
        Some(BigInt::from(1000_0000i64)),
        "DefaultNotaryAssistedAttributeFee"
    );
    assert_eq!(
        storage_key_int(
            &snapshot,
            crate::PolicyContract::milliseconds_per_block_key()
        ),
        Some(BigInt::from(settings.milliseconds_per_block)),
        "MillisecondsPerBlock migrates from ProtocolSettings"
    );
    assert_eq!(
        storage_key_int(
            &snapshot,
            crate::PolicyContract::max_valid_until_block_increment_key(),
        ),
        Some(BigInt::from(settings.max_valid_until_block_increment)),
        "MaxValidUntilBlockIncrement migrates from ProtocolSettings"
    );
    assert_eq!(
        storage_key_int(&snapshot, crate::PolicyContract::max_traceable_blocks_key()),
        Some(BigInt::from(settings.max_traceable_blocks)),
        "MaxTraceableBlocks migrates from ProtocolSettings"
    );

    // Notary's own ActiveIn seeding runs inside ContractManagement
    // OnPersist, matching C# InitializeAsync(HF_Echidna).
    let notary_initialize_seed =
        storage_key_int(&snapshot, crate::Notary::max_not_valid_before_delta_key());
    assert_eq!(notary_initialize_seed, Some(BigInt::from(140)));
}

/// The HF_Faun activation block: Policy's Faun re-initialization
/// (PolicyContract.cs:154-168) converts the stored exec-fee factor to
/// pico-GAS units and stamps blocked accounts with the persisting block's
/// timestamp; Treasury (ActiveIn = Faun) deploys.
#[test]
fn faun_block_reinitializes_policy_and_deploys_treasury() {
    let settings = settings_with(&[(Hardfork::HfFaun, 50)]);
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
    // Genesis: Policy's ActiveIn seeds (the pipeline's initialize pass) +
    // the deployment records.
    NativeContract::initialize(&crate::PolicyContract::new(), &mut engine).expect("policy init");
    NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("genesis");
    // A pre-Faun blocked account (empty-bytes record).
    let blocked = UInt160::from_bytes(&[0x77; 20]).unwrap();
    let blocked_key = crate::PolicyContract::blocked_account_key(&blocked);
    snapshot.add(blocked_key.clone(), StorageItem::from_bytes(Vec::new()));

    let timestamp: u64 = 1_700_000_000_123;
    let mut engine = on_persist_engine(&snapshot, &settings, 50, timestamp);
    NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("faun block");

    // ExecFeeFactor: 30 datoshi -> 300000 pico-GAS units.
    assert_eq!(
        storage_key_int(&snapshot, crate::PolicyContract::exec_fee_factor_key()),
        Some(BigInt::from(30i64 * 10_000))
    );
    // The blocked account now carries the persisting block's timestamp.
    assert_eq!(
        storage_key_int(&snapshot, blocked_key),
        Some(BigInt::from(timestamp))
    );
    // Treasury deploys at Faun.
    let treasury =
        ContractManagement::get_contract_from_snapshot(&snapshot, &crate::Treasury::script_hash())
            .unwrap()
            .expect("Treasury deploys at Faun");
    assert_eq!(treasury.update_counter, 0);
    let kinds: HashMap<Vec<u8>, String> = engine
        .notifications()
        .iter()
        .map(|n| {
            (
                n.state()[0].as_bytes().unwrap().to_vec(),
                n.event_name.clone(),
            )
        })
        .collect();
    assert_eq!(
        kinds.get(&crate::Treasury::script_hash().to_bytes().to_vec()),
        Some(&"Deploy".to_string())
    );
}

/// C# PolicyContract.cs:155-157: the Faun exec-fee-factor conversion
/// requires Policy to have been initialized ("Policy was not initialized").
#[test]
fn faun_reinitialization_faults_when_policy_was_never_initialized() {
    let settings = settings_with(&[(Hardfork::HfFaun, 50)]);
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = on_persist_engine(&snapshot, &settings, 50, 1);
    let result =
        crate::PolicyContract::new().initialize_for_hardfork(&mut engine, Hardfork::HfFaun);
    assert!(result.is_err(), "missing exec-fee factor must fault");
}
