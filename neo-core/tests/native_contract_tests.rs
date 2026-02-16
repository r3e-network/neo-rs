use neo_core::hardfork::{Hardfork, HardforkManager};
use neo_core::ledger::{Block, BlockHeader, create_genesis_block};
use neo_core::network::p2p::payloads::Witness;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_state::ContractState;
use neo_core::smart_contract::i_interoperable::IInteroperable;
use neo_core::smart_contract::native::{
    ContractManagement, CryptoLib, GasToken, IHardforkActivable, LedgerContract, NativeContract,
    NativeRegistry, NeoToken, Notary, OracleContract, PolicyContract, RoleManagement, StdLib,
    TreasuryContract, is_active_for,
};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::{UInt160, UInt256};
use neo_vm::{OpCode, ScriptBuilder, VMState};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

const TEST_GAS_LIMIT: i64 = 3_000_000_000;

fn protocol_settings_all_active() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    for hardfork in HardforkManager::all() {
        hardforks.insert(hardfork, 0);
    }
    settings.hardforks = hardforks;
    settings
}

fn make_snapshot_with_genesis(settings: &ProtocolSettings) -> Arc<DataCache> {
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = create_genesis_block(settings);

    let mut on_persist = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("on persist engine");
    on_persist.native_on_persist().expect("native on persist");

    let mut post_persist = ApplicationEngine::new(
        TriggerType::PostPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("post persist engine");
    post_persist
        .native_post_persist()
        .expect("native post persist");

    snapshot
}

fn emit_dynamic_call(
    builder: &mut ScriptBuilder,
    contract_hash: &UInt160,
    method: &str,
    args: &[Vec<u8>],
    call_flags: CallFlags,
) {
    for arg in args {
        builder.emit_push_byte_array(arg);
    }
    builder.emit_push_int(args.len() as i64);
    builder.emit_opcode(OpCode::PACK);
    builder.emit_push_int(call_flags.bits() as i64);
    builder.emit_push_string(method);
    builder.emit_push_byte_array(&contract_hash.to_bytes());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call syscall");
}

fn call_get_contract(
    snapshot: Arc<DataCache>,
    settings: ProtocolSettings,
    persisting_block: Block,
    hash: UInt160,
) -> ContractState {
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        Some(persisting_block),
        settings,
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    emit_dynamic_call(
        &mut script,
        &ContractManagement::new().hash(),
        "getContract",
        &[hash.to_bytes()],
        CallFlags::ALL,
    );
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");
    assert_eq!(engine.state(), VMState::HALT);

    let item = engine.result_stack().peek(0).expect("result item").clone();
    let mut state = ContractState::default();
    let _ = state.from_stack_item(item);
    state
}

fn make_block(index: u32) -> Block {
    let header = BlockHeader::new(
        0,
        UInt256::zero(),
        UInt256::zero(),
        0,
        0,
        index,
        0,
        UInt160::zero(),
        vec![Witness::empty()],
    );
    Block::new(header, Vec::new())
}

#[derive(Debug, Clone, Copy)]
struct TestActivable {
    active_in: Option<Hardfork>,
    deprecated_in: Option<Hardfork>,
}

impl IHardforkActivable for TestActivable {
    fn active_in(&self) -> Option<Hardfork> {
        self.active_in
    }

    fn deprecated_in(&self) -> Option<Hardfork> {
        self.deprecated_in
    }
}

#[test]
fn test_active_deprecated_in() {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfAspidochelone, 0);
    hardforks.insert(Hardfork::HfBasilisk, 0);
    hardforks.insert(Hardfork::HfCockatrice, 20);
    settings.hardforks = hardforks;

    let active_only = TestActivable {
        active_in: Some(Hardfork::HfCockatrice),
        deprecated_in: None,
    };
    assert!(!is_active_for(
        &active_only,
        |hf, h| settings.is_hardfork_enabled(hf, h),
        1
    ));
    assert!(is_active_for(
        &active_only,
        |hf, h| settings.is_hardfork_enabled(hf, h),
        20
    ));

    let deprecated_only = TestActivable {
        active_in: None,
        deprecated_in: Some(Hardfork::HfCockatrice),
    };
    assert!(is_active_for(
        &deprecated_only,
        |hf, h| settings.is_hardfork_enabled(hf, h),
        1
    ));
    assert!(!is_active_for(
        &deprecated_only,
        |hf, h| settings.is_hardfork_enabled(hf, h),
        20
    ));
}

#[test]
fn test_active_deprecated_in_role_management() {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfAspidochelone, 0);
    hardforks.insert(Hardfork::HfBasilisk, 0);
    hardforks.insert(Hardfork::HfEchidna, 20);
    settings.hardforks = hardforks;

    let role_mgmt = RoleManagement::new();
    let before = role_mgmt
        .contract_state(&settings, 19)
        .expect("contract state before");
    let after = role_mgmt
        .contract_state(&settings, 20)
        .expect("contract state after");

    assert_eq!(before.manifest.abi.events.len(), 1);
    assert_eq!(before.manifest.abi.events[0].parameters.len(), 2);
    assert_eq!(after.manifest.abi.events.len(), 1);
    assert_eq!(after.manifest.abi.events[0].parameters.len(), 4);
}

#[test]
fn test_get_contract() {
    let registry = NativeRegistry::new();
    let neo = NeoToken::new();
    let contract = registry.get(&neo.hash()).expect("contract");
    assert_eq!(contract.hash(), neo.hash());
    assert_eq!(contract.name(), neo.name());
}

#[test]
fn test_is_initialize_block() {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfAspidochelone, 0);
    hardforks.insert(Hardfork::HfBasilisk, 0);
    hardforks.insert(Hardfork::HfCockatrice, 20);
    hardforks.insert(Hardfork::HfDomovoi, 30);
    hardforks.insert(Hardfork::HfEchidna, 40);
    settings.hardforks = hardforks;

    let crypto = CryptoLib::new();
    let (is_init, hardforks) = crypto.is_initialize_block(&settings, 0);
    assert!(is_init);
    assert!(hardforks.is_empty());

    let (is_init, hardforks) = crypto.is_initialize_block(&settings, 1);
    assert!(!is_init);
    assert!(hardforks.is_empty());

    let (is_init, hardforks) = crypto.is_initialize_block(&settings, 20);
    assert!(is_init);
    assert_eq!(hardforks, vec![Hardfork::HfCockatrice]);
}

#[test]
fn test_genesis_nep17_manifest() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1);

    for hash in [NeoToken::new().hash(), GasToken::new().hash()] {
        let state = call_get_contract(Arc::clone(&snapshot), settings.clone(), block.clone(), hash);
        assert!(
            state
                .manifest
                .supported_standards
                .iter()
                .any(|s| s == "NEP-17"),
            "missing NEP-17 for {hash}"
        );
        assert!(
            state
                .manifest
                .abi
                .events
                .iter()
                .any(|e| e.name == "Transfer"),
            "missing Transfer event for {hash}"
        );
    }
}

#[test]
fn test_native_contract_id() {
    assert_eq!(ContractManagement::new().id(), -1);
    assert_eq!(StdLib::new().id(), -2);
    assert_eq!(CryptoLib::new().id(), -3);
    assert_eq!(LedgerContract::new().id(), -4);
    assert_eq!(NeoToken::new().id(), -5);
    assert_eq!(GasToken::new().id(), -6);
    assert_eq!(PolicyContract::new().id(), -7);
    assert_eq!(RoleManagement::new().id(), -8);
    assert_eq!(OracleContract::new().id(), -9);
    assert_eq!(Notary::new().id(), -10);
    assert_eq!(TreasuryContract::new().id(), -11);
}

fn expected_native_states() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "ContractManagement",
            r#"{"id":-1,"updatecounter":0,"hash":"0xfffdc93764dbaddd97c48f252a53ea4643faa3fd","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dA","checksum":3581846399},"manifest":{"name":"ContractManagement","groups":[],"features":{},"supportedstandards":[],"abi":{"methods":[{"name":"deploy","parameters":[{"name":"nefFile","type":"ByteArray"},{"name":"manifest","type":"ByteArray"}],"returntype":"Array","offset":0,"safe":false},{"name":"deploy","parameters":[{"name":"nefFile","type":"ByteArray"},{"name":"manifest","type":"ByteArray"},{"name":"data","type":"Any"}],"returntype":"Array","offset":7,"safe":false},{"name":"destroy","parameters":[],"returntype":"Void","offset":14,"safe":false},{"name":"getContract","parameters":[{"name":"hash","type":"Hash160"}],"returntype":"Array","offset":21,"safe":true},{"name":"getContractById","parameters":[{"name":"id","type":"Integer"}],"returntype":"Array","offset":28,"safe":true},{"name":"getContractHashes","parameters":[],"returntype":"InteropInterface","offset":35,"safe":true},{"name":"getMinimumDeploymentFee","parameters":[],"returntype":"Integer","offset":42,"safe":true},{"name":"hasMethod","parameters":[{"name":"hash","type":"Hash160"},{"name":"method","type":"String"},{"name":"pcount","type":"Integer"}],"returntype":"Boolean","offset":49,"safe":true},{"name":"isContract","parameters":[{"name":"hash","type":"Hash160"}],"returntype":"Boolean","offset":56,"safe":true},{"name":"setMinimumDeploymentFee","parameters":[{"name":"value","type":"Integer"}],"returntype":"Void","offset":63,"safe":false},{"name":"update","parameters":[{"name":"nefFile","type":"ByteArray"},{"name":"manifest","type":"ByteArray"}],"returntype":"Void","offset":70,"safe":false},{"name":"update","parameters":[{"name":"nefFile","type":"ByteArray"},{"name":"manifest","type":"ByteArray"},{"name":"data","type":"Any"}],"returntype":"Void","offset":77,"safe":false}],"events":[{"name":"Deploy","parameters":[{"name":"Hash","type":"Hash160"}]},{"name":"Update","parameters":[{"name":"Hash","type":"Hash160"}]},{"name":"Destroy","parameters":[{"name":"Hash","type":"Hash160"}]}]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "StdLib",
            r#"{"id":-2,"updatecounter":0,"hash":"0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQA==","checksum":2426471238},"manifest":{"name":"StdLib","groups":[],"features":{},"supportedstandards":[],"abi":{"methods":[{"name":"atoi","parameters":[{"name":"value","type":"String"}],"returntype":"Integer","offset":0,"safe":true},{"name":"atoi","parameters":[{"name":"value","type":"String"},{"name":"base","type":"Integer"}],"returntype":"Integer","offset":7,"safe":true},{"name":"base58CheckDecode","parameters":[{"name":"s","type":"String"}],"returntype":"ByteArray","offset":14,"safe":true},{"name":"base58CheckEncode","parameters":[{"name":"data","type":"ByteArray"}],"returntype":"String","offset":21,"safe":true},{"name":"base58Decode","parameters":[{"name":"s","type":"String"}],"returntype":"ByteArray","offset":28,"safe":true},{"name":"base58Encode","parameters":[{"name":"data","type":"ByteArray"}],"returntype":"String","offset":35,"safe":true},{"name":"base64Decode","parameters":[{"name":"s","type":"String"}],"returntype":"ByteArray","offset":42,"safe":true},{"name":"base64Encode","parameters":[{"name":"data","type":"ByteArray"}],"returntype":"String","offset":49,"safe":true},{"name":"base64UrlDecode","parameters":[{"name":"s","type":"String"}],"returntype":"String","offset":56,"safe":true},{"name":"base64UrlEncode","parameters":[{"name":"data","type":"String"}],"returntype":"String","offset":63,"safe":true},{"name":"deserialize","parameters":[{"name":"data","type":"ByteArray"}],"returntype":"Any","offset":70,"safe":true},{"name":"hexDecode","parameters":[{"name":"str","type":"String"}],"returntype":"ByteArray","offset":77,"safe":true},{"name":"hexEncode","parameters":[{"name":"bytes","type":"ByteArray"}],"returntype":"String","offset":84,"safe":true},{"name":"itoa","parameters":[{"name":"value","type":"Integer"}],"returntype":"String","offset":91,"safe":true},{"name":"itoa","parameters":[{"name":"value","type":"Integer"},{"name":"base","type":"Integer"}],"returntype":"String","offset":98,"safe":true},{"name":"jsonDeserialize","parameters":[{"name":"json","type":"ByteArray"}],"returntype":"Any","offset":105,"safe":true},{"name":"jsonSerialize","parameters":[{"name":"item","type":"Any"}],"returntype":"ByteArray","offset":112,"safe":true},{"name":"memoryCompare","parameters":[{"name":"str1","type":"ByteArray"},{"name":"str2","type":"ByteArray"}],"returntype":"Integer","offset":119,"safe":true},{"name":"memorySearch","parameters":[{"name":"mem","type":"ByteArray"},{"name":"value","type":"ByteArray"}],"returntype":"Integer","offset":126,"safe":true},{"name":"memorySearch","parameters":[{"name":"mem","type":"ByteArray"},{"name":"value","type":"ByteArray"},{"name":"start","type":"Integer"}],"returntype":"Integer","offset":133,"safe":true},{"name":"memorySearch","parameters":[{"name":"mem","type":"ByteArray"},{"name":"value","type":"ByteArray"},{"name":"start","type":"Integer"},{"name":"backward","type":"Boolean"}],"returntype":"Integer","offset":140,"safe":true},{"name":"serialize","parameters":[{"name":"item","type":"Any"}],"returntype":"ByteArray","offset":147,"safe":true},{"name":"strLen","parameters":[{"name":"str","type":"String"}],"returntype":"Integer","offset":154,"safe":true},{"name":"stringSplit","parameters":[{"name":"str","type":"String"},{"name":"separator","type":"String"}],"returntype":"Array","offset":161,"safe":true},{"name":"stringSplit","parameters":[{"name":"str","type":"String"},{"name":"separator","type":"String"},{"name":"removeEmptyEntries","type":"Boolean"}],"returntype":"Array","offset":168,"safe":true}],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "CryptoLib",
            r#"{"id":-3,"updatecounter":0,"hash":"0x726cb6e0cd8628a1350a611384688911ab75f51b","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQA==","checksum":174904780},"manifest":{"name":"CryptoLib","groups":[],"features":{},"supportedstandards":[],"abi":{"methods":[{"name":"bls12381Add","parameters":[{"name":"x","type":"InteropInterface"},{"name":"y","type":"InteropInterface"}],"returntype":"InteropInterface","offset":0,"safe":true},{"name":"bls12381Deserialize","parameters":[{"name":"data","type":"ByteArray"}],"returntype":"InteropInterface","offset":7,"safe":true},{"name":"bls12381Equal","parameters":[{"name":"x","type":"InteropInterface"},{"name":"y","type":"InteropInterface"}],"returntype":"Boolean","offset":14,"safe":true},{"name":"bls12381Mul","parameters":[{"name":"x","type":"InteropInterface"},{"name":"mul","type":"ByteArray"},{"name":"neg","type":"Boolean"}],"returntype":"InteropInterface","offset":21,"safe":true},{"name":"bls12381Pairing","parameters":[{"name":"g1","type":"InteropInterface"},{"name":"g2","type":"InteropInterface"}],"returntype":"InteropInterface","offset":28,"safe":true},{"name":"bls12381Serialize","parameters":[{"name":"g","type":"InteropInterface"}],"returntype":"ByteArray","offset":35,"safe":true},{"name":"keccak256","parameters":[{"name":"data","type":"ByteArray"}],"returntype":"ByteArray","offset":42,"safe":true},{"name":"murmur32","parameters":[{"name":"data","type":"ByteArray"},{"name":"seed","type":"Integer"}],"returntype":"ByteArray","offset":49,"safe":true},{"name":"recoverSecp256K1","parameters":[{"name":"messageHash","type":"ByteArray"},{"name":"signature","type":"ByteArray"}],"returntype":"ByteArray","offset":56,"safe":true},{"name":"ripemd160","parameters":[{"name":"data","type":"ByteArray"}],"returntype":"ByteArray","offset":63,"safe":true},{"name":"sha256","parameters":[{"name":"data","type":"ByteArray"}],"returntype":"ByteArray","offset":70,"safe":true},{"name":"verifyWithECDsa","parameters":[{"name":"message","type":"ByteArray"},{"name":"pubkey","type":"ByteArray"},{"name":"signature","type":"ByteArray"},{"name":"curveHash","type":"Integer"}],"returntype":"Boolean","offset":77,"safe":true},{"name":"verifyWithEd25519","parameters":[{"name":"message","type":"ByteArray"},{"name":"pubkey","type":"ByteArray"},{"name":"signature","type":"ByteArray"}],"returntype":"Boolean","offset":84,"safe":true}],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "LedgerContract",
            r#"{"id":-4,"updatecounter":0,"hash":"0xda65b600f7124ce6c79950c1772a36403104f2be","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0A=","checksum":1110259869},"manifest":{"name":"LedgerContract","groups":[],"features":{},"supportedstandards":[],"abi":{"methods":[{"name":"currentHash","parameters":[],"returntype":"Hash256","offset":0,"safe":true},{"name":"currentIndex","parameters":[],"returntype":"Integer","offset":7,"safe":true},{"name":"getBlock","parameters":[{"name":"indexOrHash","type":"ByteArray"}],"returntype":"Array","offset":14,"safe":true},{"name":"getTransaction","parameters":[{"name":"hash","type":"Hash256"}],"returntype":"Array","offset":21,"safe":true},{"name":"getTransactionFromBlock","parameters":[{"name":"blockIndexOrHash","type":"ByteArray"},{"name":"txIndex","type":"Integer"}],"returntype":"Array","offset":28,"safe":true},{"name":"getTransactionHeight","parameters":[{"name":"hash","type":"Hash256"}],"returntype":"Integer","offset":35,"safe":true},{"name":"getTransactionSigners","parameters":[{"name":"hash","type":"Hash256"}],"returntype":"Array","offset":42,"safe":true},{"name":"getTransactionVMState","parameters":[{"name":"hash","type":"Hash256"}],"returntype":"Integer","offset":49,"safe":true}],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "NeoToken",
            r#"{"id":-5,"updatecounter":0,"hash":"0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dA","checksum":1991619121},"manifest":{"name":"NeoToken","groups":[],"features":{},"supportedstandards":["NEP-17","NEP-27"],"abi":{"methods":[{"name":"balanceOf","parameters":[{"name":"account","type":"Hash160"}],"returntype":"Integer","offset":0,"safe":true},{"name":"decimals","parameters":[],"returntype":"Integer","offset":7,"safe":true},{"name":"getAccountState","parameters":[{"name":"account","type":"Hash160"}],"returntype":"Array","offset":14,"safe":true},{"name":"getAllCandidates","parameters":[],"returntype":"InteropInterface","offset":21,"safe":true},{"name":"getCandidateVote","parameters":[{"name":"pubKey","type":"PublicKey"}],"returntype":"Integer","offset":28,"safe":true},{"name":"getCandidates","parameters":[],"returntype":"Array","offset":35,"safe":true},{"name":"getCommittee","parameters":[],"returntype":"Array","offset":42,"safe":true},{"name":"getCommitteeAddress","parameters":[],"returntype":"Hash160","offset":49,"safe":true},{"name":"getGasPerBlock","parameters":[],"returntype":"Integer","offset":56,"safe":true},{"name":"getNextBlockValidators","parameters":[],"returntype":"Array","offset":63,"safe":true},{"name":"getRegisterPrice","parameters":[],"returntype":"Integer","offset":70,"safe":true},{"name":"onNEP17Payment","parameters":[{"name":"from","type":"Hash160"},{"name":"amount","type":"Integer"},{"name":"data","type":"Any"}],"returntype":"Void","offset":77,"safe":false},{"name":"registerCandidate","parameters":[{"name":"pubkey","type":"PublicKey"}],"returntype":"Boolean","offset":84,"safe":false},{"name":"setGasPerBlock","parameters":[{"name":"gasPerBlock","type":"Integer"}],"returntype":"Void","offset":91,"safe":false},{"name":"setRegisterPrice","parameters":[{"name":"registerPrice","type":"Integer"}],"returntype":"Void","offset":98,"safe":false},{"name":"symbol","parameters":[],"returntype":"String","offset":105,"safe":true},{"name":"totalSupply","parameters":[],"returntype":"Integer","offset":112,"safe":true},{"name":"transfer","parameters":[{"name":"from","type":"Hash160"},{"name":"to","type":"Hash160"},{"name":"amount","type":"Integer"},{"name":"data","type":"Any"}],"returntype":"Boolean","offset":119,"safe":false},{"name":"unclaimedGas","parameters":[{"name":"account","type":"Hash160"},{"name":"end","type":"Integer"}],"returntype":"Integer","offset":126,"safe":true},{"name":"unregisterCandidate","parameters":[{"name":"pubkey","type":"PublicKey"}],"returntype":"Boolean","offset":133,"safe":false},{"name":"vote","parameters":[{"name":"account","type":"Hash160"},{"name":"voteTo","type":"PublicKey"}],"returntype":"Boolean","offset":140,"safe":false}],"events":[{"name":"Transfer","parameters":[{"name":"from","type":"Hash160"},{"name":"to","type":"Hash160"},{"name":"amount","type":"Integer"}]},{"name":"CandidateStateChanged","parameters":[{"name":"pubkey","type":"PublicKey"},{"name":"registered","type":"Boolean"},{"name":"votes","type":"Integer"}]},{"name":"Vote","parameters":[{"name":"account","type":"Hash160"},{"name":"from","type":"PublicKey"},{"name":"to","type":"PublicKey"},{"name":"amount","type":"Integer"}]},{"name":"CommitteeChanged","parameters":[{"name":"old","type":"Array"},{"name":"new","type":"Array"}]}]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "GasToken",
            r#"{"id":-6,"updatecounter":0,"hash":"0xd2a4cff31913016155e38e474a2c06d08be276cf","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0A=","checksum":2663858513},"manifest":{"name":"GasToken","groups":[],"features":{},"supportedstandards":["NEP-17"],"abi":{"methods":[{"name":"balanceOf","parameters":[{"name":"account","type":"Hash160"}],"returntype":"Integer","offset":0,"safe":true},{"name":"decimals","parameters":[],"returntype":"Integer","offset":7,"safe":true},{"name":"symbol","parameters":[],"returntype":"String","offset":14,"safe":true},{"name":"totalSupply","parameters":[],"returntype":"Integer","offset":21,"safe":true},{"name":"transfer","parameters":[{"name":"from","type":"Hash160"},{"name":"to","type":"Hash160"},{"name":"amount","type":"Integer"},{"name":"data","type":"Any"}],"returntype":"Boolean","offset":28,"safe":false}],"events":[{"name":"Transfer","parameters":[{"name":"from","type":"Hash160"},{"name":"to","type":"Hash160"},{"name":"amount","type":"Integer"}]}]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "PolicyContract",
            r#"{"id":-7,"updatecounter":0,"hash":"0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0A=","checksum":2681632925},"manifest":{"name":"PolicyContract","groups":[],"features":{},"supportedstandards":[],"abi":{"methods":[{"name":"blockAccount","parameters":[{"name":"account","type":"Hash160"}],"returntype":"Boolean","offset":0,"safe":false},{"name":"getAttributeFee","parameters":[{"name":"attributeType","type":"Integer"}],"returntype":"Integer","offset":7,"safe":true},{"name":"getBlockedAccounts","parameters":[],"returntype":"InteropInterface","offset":14,"safe":true},{"name":"getExecFeeFactor","parameters":[],"returntype":"Integer","offset":21,"safe":true},{"name":"getExecPicoFeeFactor","parameters":[],"returntype":"Integer","offset":28,"safe":true},{"name":"getFeePerByte","parameters":[],"returntype":"Integer","offset":35,"safe":true},{"name":"getMaxTraceableBlocks","parameters":[],"returntype":"Integer","offset":42,"safe":true},{"name":"getMaxValidUntilBlockIncrement","parameters":[],"returntype":"Integer","offset":49,"safe":true},{"name":"getMillisecondsPerBlock","parameters":[],"returntype":"Integer","offset":56,"safe":true},{"name":"getStoragePrice","parameters":[],"returntype":"Integer","offset":63,"safe":true},{"name":"getWhitelistFeeContracts","parameters":[],"returntype":"InteropInterface","offset":70,"safe":true},{"name":"isBlocked","parameters":[{"name":"account","type":"Hash160"}],"returntype":"Boolean","offset":77,"safe":true},{"name":"recoverFund","parameters":[{"name":"account","type":"Hash160"},{"name":"token","type":"Hash160"}],"returntype":"Boolean","offset":84,"safe":false},{"name":"removeWhitelistFeeContract","parameters":[{"name":"contractHash","type":"Hash160"},{"name":"method","type":"String"},{"name":"argCount","type":"Integer"}],"returntype":"Void","offset":91,"safe":false},{"name":"setAttributeFee","parameters":[{"name":"attributeType","type":"Integer"},{"name":"value","type":"Integer"}],"returntype":"Void","offset":98,"safe":false},{"name":"setExecFeeFactor","parameters":[{"name":"value","type":"Integer"}],"returntype":"Void","offset":105,"safe":false},{"name":"setFeePerByte","parameters":[{"name":"value","type":"Integer"}],"returntype":"Void","offset":112,"safe":false},{"name":"setMaxTraceableBlocks","parameters":[{"name":"value","type":"Integer"}],"returntype":"Void","offset":119,"safe":false},{"name":"setMaxValidUntilBlockIncrement","parameters":[{"name":"value","type":"Integer"}],"returntype":"Void","offset":126,"safe":false},{"name":"setMillisecondsPerBlock","parameters":[{"name":"value","type":"Integer"}],"returntype":"Void","offset":133,"safe":false},{"name":"setStoragePrice","parameters":[{"name":"value","type":"Integer"}],"returntype":"Void","offset":140,"safe":false},{"name":"setWhitelistFeeContract","parameters":[{"name":"contractHash","type":"Hash160"},{"name":"method","type":"String"},{"name":"argCount","type":"Integer"},{"name":"fixedFee","type":"Integer"}],"returntype":"Void","offset":147,"safe":false},{"name":"unblockAccount","parameters":[{"name":"account","type":"Hash160"}],"returntype":"Boolean","offset":154,"safe":false}],"events":[{"name":"MillisecondsPerBlockChanged","parameters":[{"name":"old","type":"Integer"},{"name":"new","type":"Integer"}]},{"name":"WhitelistFeeChanged","parameters":[{"name":"contract","type":"Hash160"},{"name":"method","type":"String"},{"name":"argCount","type":"Integer"},{"name":"fee","type":"Any"}]},{"name":"RecoveredFund","parameters":[{"name":"account","type":"Hash160"}]}]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "RoleManagement",
            r#"{"id":-8,"updatecounter":0,"hash":"0x49cf4e5378ffcd4dec034fd98a174c5491e395e2","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0A=","checksum":983638438},"manifest":{"name":"RoleManagement","groups":[],"features":{},"supportedstandards":[],"abi":{"methods":[{"name":"designateAsRole","parameters":[{"name":"role","type":"Integer"},{"name":"nodes","type":"Array"}],"returntype":"Void","offset":0,"safe":false},{"name":"getDesignatedByRole","parameters":[{"name":"role","type":"Integer"},{"name":"index","type":"Integer"}],"returntype":"Array","offset":7,"safe":true}],"events":[{"name":"Designation","parameters":[{"name":"Role","type":"Integer"},{"name":"BlockIndex","type":"Integer"},{"name":"Old","type":"Array"},{"name":"New","type":"Array"}]}]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "OracleContract",
            r#"{"id":-9,"updatecounter":0,"hash":"0xfe924b7cfe89ddd271abaf7210a80a7e11178758","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0A=","checksum":2663858513},"manifest":{"name":"OracleContract","groups":[],"features":{},"supportedstandards":["NEP-30"],"abi":{"methods":[{"name":"finish","parameters":[],"returntype":"Void","offset":0,"safe":false},{"name":"getPrice","parameters":[],"returntype":"Integer","offset":7,"safe":true},{"name":"request","parameters":[{"name":"url","type":"String"},{"name":"filter","type":"String"},{"name":"callback","type":"String"},{"name":"userData","type":"Any"},{"name":"gasForResponse","type":"Integer"}],"returntype":"Void","offset":14,"safe":false},{"name":"setPrice","parameters":[{"name":"price","type":"Integer"}],"returntype":"Void","offset":21,"safe":false},{"name":"verify","parameters":[],"returntype":"Boolean","offset":28,"safe":true}],"events":[{"name":"OracleRequest","parameters":[{"name":"Id","type":"Integer"},{"name":"RequestContract","type":"Hash160"},{"name":"Url","type":"String"},{"name":"Filter","type":"String"}]},{"name":"OracleResponse","parameters":[{"name":"Id","type":"Integer"},{"name":"OriginalTx","type":"Hash256"}]}]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "Notary",
            r#"{"id":-10,"updatecounter":0,"hash":"0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0AQQRr3e2dAEEEa93tnQBBBGvd7Z0A=","checksum":1110259869},"manifest":{"name":"Notary","groups":[],"features":{},"supportedstandards":["NEP-27","NEP-30"],"abi":{"methods":[{"name":"balanceOf","parameters":[{"name":"account","type":"Hash160"}],"returntype":"Integer","offset":0,"safe":true},{"name":"expirationOf","parameters":[{"name":"account","type":"Hash160"}],"returntype":"Integer","offset":7,"safe":true},{"name":"getMaxNotValidBeforeDelta","parameters":[],"returntype":"Integer","offset":14,"safe":true},{"name":"lockDepositUntil","parameters":[{"name":"account","type":"Hash160"},{"name":"till","type":"Integer"}],"returntype":"Boolean","offset":21,"safe":false},{"name":"onNEP17Payment","parameters":[{"name":"from","type":"Hash160"},{"name":"amount","type":"Integer"},{"name":"data","type":"Any"}],"returntype":"Void","offset":28,"safe":false},{"name":"setMaxNotValidBeforeDelta","parameters":[{"name":"value","type":"Integer"}],"returntype":"Void","offset":35,"safe":false},{"name":"verify","parameters":[{"name":"signature","type":"ByteArray"}],"returntype":"Boolean","offset":42,"safe":true},{"name":"withdraw","parameters":[{"name":"from","type":"Hash160"},{"name":"to","type":"Hash160"}],"returntype":"Boolean","offset":49,"safe":false}],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
        (
            "Treasury",
            r#"{"id":-11,"updatecounter":0,"hash":"0x156326f25b1b5d839a4d326aeaa75383c9563ac1","nef":{"magic":860243278,"compiler":"neo-core-v3.0","source":"","tokens":[],"script":"EEEa93tnQBBBGvd7Z0AQQRr3e2dA","checksum":1592866325},"manifest":{"name":"Treasury","groups":[],"features":{},"supportedstandards":["NEP-26","NEP-27","NEP-30"],"abi":{"methods":[{"name":"onNEP11Payment","parameters":[{"name":"from","type":"Hash160"},{"name":"amount","type":"Integer"},{"name":"tokenId","type":"ByteArray"},{"name":"data","type":"Any"}],"returntype":"Void","offset":0,"safe":true},{"name":"onNEP17Payment","parameters":[{"name":"from","type":"Hash160"},{"name":"amount","type":"Integer"},{"name":"data","type":"Any"}],"returntype":"Void","offset":7,"safe":true},{"name":"verify","parameters":[],"returntype":"Boolean","offset":14,"safe":true}],"events":[]},"permissions":[{"contract":"*","methods":"*"}],"trusts":[],"extra":null}}"#,
        ),
    ]
}

#[test]
fn test_genesis_native_state() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let block = make_block(1);

    for (name, expected) in expected_native_states() {
        let registry = NativeRegistry::new();
        let contract = registry
            .get_by_name(name)
            .unwrap_or_else(|| panic!("missing native contract {name}"));

        let state = call_get_contract(
            Arc::clone(&snapshot),
            settings.clone(),
            block.clone(),
            contract.hash(),
        );
        let actual = state.to_json().expect("state json");
        let expected_json: Value = serde_json::from_str(expected).expect("expected json");
        assert_eq!(actual, expected_json, "{name} is wrong");
    }
}
