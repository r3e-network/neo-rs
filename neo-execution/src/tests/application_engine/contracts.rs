use super::*;
use crate::native_contract::OracleRequestDetails;
use crate::native_contract_provider::lock_native_provider;
use crate::native_contract_provider::{NativeContractLookup, NativeContractProvider};
use neo_crypto::{ECCurve, ECPoint};
use neo_manifest::{
    ContractAbi, ContractGroup, ContractManifest, ContractMethodDescriptor,
    ContractParameterDefinition, ContractPermission, NefFile, WildCardContainer,
};
use neo_payloads::{OracleResponse, Signer, Transaction, TransactionAttribute};
use neo_primitives::{ContractParameterType, OracleResponseCode, WitnessScope};
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use parking_lot::Mutex as PlMutex;
use std::collections::HashMap;

fn lock_provider() -> crate::native_contract_provider::NativeProviderTestGuard {
    lock_native_provider()
}

struct BlockingPolicy {
    blocked_hash: UInt160,
}

impl NativeContract for BlockingPolicy {
    fn id(&self) -> i32 {
        -7
    }

    fn hash(&self) -> UInt160 {
        UInt160::from_bytes(&[0xCC; 20]).expect("policy hash")
    }

    fn name(&self) -> &str {
        "PolicyContract"
    }

    fn methods(&self) -> &[crate::NativeMethod] {
        &[]
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Err(CoreError::invalid_operation("test policy is metadata-only"))
    }

    fn is_contract_blocked(
        &self,
        _snapshot: &DataCache,
        contract_hash: &UInt160,
    ) -> CoreResult<bool> {
        Ok(contract_hash == &self.blocked_hash)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct BlockingProvider {
    policy: Arc<BlockingPolicy>,
}

impl NativeContractProvider for BlockingProvider {
    fn get_native_contract(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        (&self.policy.hash() == hash).then(|| self.policy.clone() as Arc<dyn NativeContract>)
    }

    fn get_native_contract_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        name.eq_ignore_ascii_case("PolicyContract")
            .then(|| self.policy.clone() as Arc<dyn NativeContract>)
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        vec![self.policy.clone() as Arc<dyn NativeContract>]
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        vec![self.policy.hash()]
    }
}

fn install_blocking_policy(blocked_hash: UInt160) {
    NativeContractLookup::install_provider(Arc::new(BlockingProvider {
        policy: Arc::new(BlockingPolicy { blocked_hash }),
    }));
}

fn install_allowing_policy() {
    let never_blocked = UInt160::from_bytes(&[0xEE; 20]).expect("non-target hash");
    install_blocking_policy(never_blocked);
}

struct EmptyProvider;

impl NativeContractProvider for EmptyProvider {
    fn get_native_contract(&self, _hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        None
    }

    fn get_native_contract_by_name(&self, _name: &str) -> Option<Arc<dyn NativeContract>> {
        None
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        Vec::new()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        Vec::new()
    }
}

struct MeteredNativeContract {
    hash: UInt160,
    methods: Vec<crate::NativeMethod>,
}

impl MeteredNativeContract {
    fn new(storage_fee: i64) -> Self {
        Self {
            hash: UInt160::from_bytes(&[0xA5; 20]).expect("metered native hash"),
            methods: vec![
                crate::NativeMethod::new(
                    "metered",
                    0,
                    false,
                    CallFlags::ALL.bits(),
                    Vec::new(),
                    ContractParameterType::Void,
                )
                .with_storage_fee(storage_fee),
            ],
        }
    }
}

impl NativeContract for MeteredNativeContract {
    fn id(&self) -> i32 {
        -99
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn name(&self) -> &str {
        "MeteredNative"
    }

    fn methods(&self) -> &[crate::NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        if method == "metered" {
            Ok(Vec::new())
        } else {
            Err(CoreError::invalid_operation(format!(
                "unexpected method {method}"
            )))
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct SingleNativeProvider {
    native: Arc<dyn NativeContract>,
}

impl NativeContractProvider for SingleNativeProvider {
    fn get_native_contract(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        (&self.native.hash() == hash).then(|| self.native.clone())
    }

    fn get_native_contract_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        name.eq_ignore_ascii_case(self.native.name())
            .then(|| self.native.clone())
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        vec![self.native.clone()]
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        vec![self.native.hash()]
    }
}

struct NativeSetProvider {
    natives: Vec<Arc<dyn NativeContract>>,
}

impl NativeContractProvider for NativeSetProvider {
    fn get_native_contract(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        self.natives
            .iter()
            .find(|native| native.hash() == *hash)
            .cloned()
    }

    fn get_native_contract_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        self.natives
            .iter()
            .find(|native| name.eq_ignore_ascii_case(native.name()))
            .cloned()
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        self.natives.clone()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        self.natives.iter().map(|native| native.hash()).collect()
    }
}

struct ContractManagementNative {
    contract: ContractState,
}

impl NativeContract for ContractManagementNative {
    fn id(&self) -> i32 {
        -1
    }

    fn hash(&self) -> UInt160 {
        UInt160::from_bytes(&[0xCD; 20]).expect("contract management hash")
    }

    fn name(&self) -> &str {
        "ContractManagement"
    }

    fn methods(&self) -> &[crate::NativeMethod] {
        &[]
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Err(CoreError::invalid_operation(
            "test contract management is metadata-only",
        ))
    }

    fn lookup_contract_state(
        &self,
        _snapshot: &DataCache,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        Ok((hash == &self.contract.hash).then(|| self.contract.clone()))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct FailingCommitteeNative;

impl NativeContract for FailingCommitteeNative {
    fn id(&self) -> i32 {
        -5
    }

    fn hash(&self) -> UInt160 {
        UInt160::from_bytes(&[0xCE; 20]).expect("neo token hash")
    }

    fn name(&self) -> &str {
        "NeoToken"
    }

    fn methods(&self) -> &[crate::NativeMethod] {
        &[]
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Err(CoreError::invalid_operation(
            "test neo token is metadata-only",
        ))
    }

    fn committee_address(&self, _snapshot: &DataCache) -> CoreResult<Option<UInt160>> {
        Err(CoreError::invalid_operation(
            "captured committee provider used",
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct OracleNative {
    original_tx_id: UInt256,
}

impl NativeContract for OracleNative {
    fn id(&self) -> i32 {
        -9
    }

    fn hash(&self) -> UInt160 {
        UInt160::from_bytes(&[0xC9; 20]).expect("oracle hash")
    }

    fn name(&self) -> &str {
        "OracleContract"
    }

    fn methods(&self) -> &[crate::NativeMethod] {
        &[]
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Err(CoreError::invalid_operation(
            "test oracle contract is metadata-only",
        ))
    }

    fn oracle_request_url_full(
        &self,
        _snapshot: &DataCache,
        _id: u64,
    ) -> CoreResult<Option<OracleRequestDetails>> {
        Ok(Some(OracleRequestDetails::new(
            "https://neo.org",
            self.original_tx_id,
        )))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct FailingLedgerNative;

impl NativeContract for FailingLedgerNative {
    fn id(&self) -> i32 {
        -4
    }

    fn hash(&self) -> UInt160 {
        UInt160::from_bytes(&[0xC4; 20]).expect("ledger hash")
    }

    fn name(&self) -> &str {
        "LedgerContract"
    }

    fn methods(&self) -> &[crate::NativeMethod] {
        &[]
    }

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        _method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        Err(CoreError::invalid_operation(
            "test ledger contract is metadata-only",
        ))
    }

    fn transaction_state(
        &self,
        _snapshot: &DataCache,
        _tx_hash: &UInt256,
    ) -> CoreResult<Option<neo_payloads::TransactionState>> {
        Err(CoreError::invalid_operation(
            "captured ledger provider used",
        ))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Builds a small synthetic contract with a single `balanceOf(account)`
/// method that returns immediately. Used by the dynamic-call tests so
/// they do not depend on the GAS native contract being installed via
/// the global `NativeContractProvider`.
fn build_mock_contract(hash: UInt160) -> ContractState {
    let script = vec![OpCode::RET.byte()];
    let nef = NefFile::new("test".to_string(), script);

    let param =
        ContractParameterDefinition::new("account".to_string(), ContractParameterType::Hash160)
            .expect("parameter");

    let method = ContractMethodDescriptor::new(
        "balanceOf".to_string(),
        vec![param],
        ContractParameterType::Integer,
        0,
        true,
    )
    .expect("descriptor");

    let abi = ContractAbi::new(vec![method], Vec::new());

    let manifest = ContractManifest {
        name: "MockContract".to_string(),
        groups: Vec::new(),
        features: std::collections::HashMap::new(),
        supported_standards: Vec::new(),
        abi,
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::default(),
        extra: None,
    };

    ContractState::new(1, hash, nef, manifest)
}

#[test]
fn native_method_storage_fee_is_charged_in_datoshi() {
    let _provider_guard = lock_provider();
    let native = Arc::new(MeteredNativeContract::new(50));
    let native_hash = native.hash();
    NativeContractLookup::install_provider(Arc::new(SingleNativeProvider { native }));

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
    )
    .expect("engine");

    engine
        .call_native_contract(native_hash, "metered", &[])
        .expect("native method succeeds");

    assert_eq!(
        engine.fee_consumed(),
        50 * 100_000,
        "C# NativeContract.Invoke charges StoragePrice * StorageFee through AddFee, in datoshi"
    );
}

#[test]
fn native_call_uses_provider_captured_at_engine_creation() {
    let _provider_guard = lock_provider();
    let native = Arc::new(MeteredNativeContract::new(50));
    let native_hash = native.hash();
    let provider = Arc::new(SingleNativeProvider { native }) as Arc<dyn NativeContractProvider>;

    let mut engine = NativeContractLookup::with_scoped_provider(provider, || {
        ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
    })
    .expect("engine");

    NativeContractLookup::install_provider(Arc::new(EmptyProvider));

    engine
        .call_native_contract(native_hash, "metered", &[])
        .expect("native method should use provider captured at engine creation");

    assert_eq!(engine.fee_consumed(), 50 * 100_000);
}

#[test]
fn committee_witness_uses_provider_captured_at_engine_creation() {
    let _provider_guard = lock_provider();
    let provider = Arc::new(SingleNativeProvider {
        native: Arc::new(FailingCommitteeNative),
    }) as Arc<dyn NativeContractProvider>;

    let engine = NativeContractLookup::with_scoped_provider(provider, || {
        ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
    })
    .expect("engine");

    NativeContractLookup::install_provider(Arc::new(EmptyProvider));

    let err = engine
        .check_committee_witness()
        .expect_err("captured NeoToken provider should be used");
    assert!(
        err.to_string().contains("captured committee provider used"),
        "unexpected error: {err}"
    );
}

#[test]
fn storage_context_uses_provider_captured_at_engine_creation() {
    let _provider_guard = lock_provider();
    let contract_hash =
        UInt160::parse("0xa1b2c3d4e5f60718293a4b5c6d7e8f0102030416").expect("contract hash");
    let contract = build_mock_contract(contract_hash);
    let provider = Arc::new(SingleNativeProvider {
        native: Arc::new(ContractManagementNative {
            contract: contract.clone(),
        }),
    }) as Arc<dyn NativeContractProvider>;

    let mut engine = NativeContractLookup::with_scoped_provider(provider, || {
        ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
    })
    .expect("engine");

    NativeContractLookup::install_provider(Arc::new(EmptyProvider));

    engine
        .load_script(
            vec![OpCode::RET.byte()],
            CallFlags::ALL,
            Some(contract_hash),
        )
        .expect("load contract script");

    let context = engine
        .get_storage_context()
        .expect("captured ContractManagement provider should resolve contract state");
    assert_eq!(context.id, contract.id);
    assert!(!context.is_read_only);
}

#[test]
fn oracle_response_witness_uses_provider_captured_at_engine_creation() {
    let _provider_guard = lock_provider();
    let original_tx_id = UInt256::from_bytes(&[0x42; 32]).expect("original tx hash");
    let provider = Arc::new(NativeSetProvider {
        natives: vec![
            Arc::new(OracleNative { original_tx_id }) as Arc<dyn NativeContract>,
            Arc::new(FailingLedgerNative) as Arc<dyn NativeContract>,
        ],
    }) as Arc<dyn NativeContractProvider>;

    let delegated_signer =
        UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121324").expect("signer");
    let mut tx = Transaction::new();
    tx.set_attributes(vec![TransactionAttribute::OracleResponse(
        OracleResponse::new(7, OracleResponseCode::Success, Vec::new()),
    )]);
    tx.set_signers(vec![Signer::new(delegated_signer, WitnessScope::NONE)]);

    let engine = NativeContractLookup::with_scoped_provider(provider, || {
        ApplicationEngine::new(
            TriggerType::Application,
            Some(Arc::new(tx)),
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
    })
    .expect("engine");

    NativeContractLookup::install_provider(Arc::new(EmptyProvider));

    let err = engine
        .check_witness_hash(&delegated_signer)
        .expect_err("captured Oracle and Ledger providers should be used");
    assert!(
        err.to_string().contains("captured ledger provider used"),
        "unexpected error: {err}"
    );
}

#[test]
fn group_witness_uses_provider_captured_at_engine_creation() {
    let _provider_guard = lock_provider();
    let group_bytes =
        hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
            .expect("group public key");
    let group_point =
        ECPoint::from_bytes_with_curve(ECCurve::secp256r1(), &group_bytes).expect("group point");
    let contract_hash =
        UInt160::parse("0xa1b2c3d4e5f60718293a4b5c6d7e8f0102030426").expect("contract hash");
    let mut contract = build_mock_contract(contract_hash);
    contract
        .manifest
        .groups
        .push(ContractGroup::new(group_point.clone(), vec![0; 64]));
    let provider = Arc::new(SingleNativeProvider {
        native: Arc::new(ContractManagementNative { contract }),
    }) as Arc<dyn NativeContractProvider>;

    let witness_account =
        UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121325").expect("signer");
    let mut signer = Signer::new(witness_account, WitnessScope::CUSTOM_GROUPS);
    signer.allowed_groups.push(group_point);
    let mut tx = Transaction::new();
    tx.set_signers(vec![signer]);

    let mut engine = NativeContractLookup::with_scoped_provider(provider, || {
        ApplicationEngine::new(
            TriggerType::Application,
            Some(Arc::new(tx)),
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
        )
    })
    .expect("engine");

    NativeContractLookup::install_provider(Arc::new(EmptyProvider));

    engine
        .load_script(
            vec![OpCode::RET.byte()],
            CallFlags::READ_STATES,
            Some(contract_hash),
        )
        .expect("load grouped contract script");

    assert!(
        engine
            .check_witness_hash(&witness_account)
            .expect("captured ContractManagement provider should evaluate group"),
        "group witness must match the contract manifest exposed by the captured provider"
    );
}

#[test]
fn call_contract_uses_execution_state_script_hash_for_caller() {
    let _provider_guard = lock_provider();
    install_allowing_policy();

    let snapshot = Arc::new(DataCache::new(false));

    // Pre-load a mock contract directly into the engine so the test
    // is self-contained (does not rely on a globally-installed
    // NativeContractProvider).
    let target_hash =
        UInt160::parse("0xa1b2c3d4e5f60718293a4b5c6d7e8f0102030405").expect("target hash");
    let mut contracts: HashMap<UInt160, ContractState> = HashMap::new();
    contracts.insert(target_hash, build_mock_contract(target_hash));

    let mut engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        contracts,
        Arc::new(PlMutex::new(NativeContractsCache::default())),
        None,
    )
    .expect("engine");

    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load entry script");

    let entry_context = engine.current_context().cloned().expect("entry context");
    let vm_script_hash =
        UInt160::from_bytes(&entry_context.script_hash()).expect("entry vm script hash");
    let logical_contract_hash =
        UInt160::parse("0xc198d687cc67e244662c3b9c1325f095f8e663b1").expect("hash");
    assert_ne!(logical_contract_hash, vm_script_hash);

    let state_arc = entry_context
        .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
    state_arc.lock().script_hash = Some(logical_contract_hash);
    engine
        .refresh_context_tracking()
        .expect("refresh context tracking");

    engine
        .call_contract_dynamic(
            &target_hash,
            "balanceOf",
            CallFlags::READ_STATES | CallFlags::ALLOW_CALL,
            vec![StackItem::from_byte_string(UInt160::zero().to_bytes())],
        )
        .expect("load mock balanceOf call");

    let called_context = engine.current_context().cloned().expect("called context");
    let called_state_arc = called_context
        .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
    let called_state = called_state_arc.lock();

    assert_eq!(
        called_state.calling_script_hash,
        Some(logical_contract_hash)
    );
    assert_eq!(
        engine.get_calling_script_hash(),
        Some(logical_contract_hash)
    );
}

#[test]
fn call_contract_dynamic_rejects_policy_blocked_target() {
    let _provider_guard = lock_provider();

    let target_hash =
        UInt160::parse("0xb1b2c3d4e5f60718293a4b5c6d7e8f0102030405").expect("target hash");
    install_blocking_policy(target_hash);

    let mut contracts: HashMap<UInt160, ContractState> = HashMap::new();
    contracts.insert(target_hash, build_mock_contract(target_hash));
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        contracts,
        Arc::new(PlMutex::new(NativeContractsCache::default())),
        None,
    )
    .expect("engine");

    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load entry script");

    let err = engine
        .call_contract_dynamic(
            &target_hash,
            "balanceOf",
            CallFlags::READ_STATES | CallFlags::ALLOW_CALL,
            vec![StackItem::from_byte_string(UInt160::zero().to_bytes())],
        )
        .expect_err("blocked contract call must fault before loading callee");

    assert!(
        err.to_string()
            .contains(&format!("The contract {target_hash} has been blocked.")),
        "unexpected error: {err}"
    );
    assert_eq!(
        engine.invocation_stack().len(),
        1,
        "blocked target must not be loaded onto the invocation stack"
    );
}

#[test]
fn dynamic_contract_policy_uses_provider_captured_at_engine_creation() {
    let _provider_guard = lock_provider();

    let target_hash =
        UInt160::parse("0xb1b2c3d4e5f60718293a4b5c6d7e8f0102030415").expect("target hash");
    let provider = Arc::new(BlockingProvider {
        policy: Arc::new(BlockingPolicy {
            blocked_hash: target_hash,
        }),
    }) as Arc<dyn NativeContractProvider>;

    let mut contracts: HashMap<UInt160, ContractState> = HashMap::new();
    contracts.insert(target_hash, build_mock_contract(target_hash));

    let mut engine = NativeContractLookup::with_scoped_provider(provider, || {
        ApplicationEngine::new_with_preloaded_native(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            contracts,
            Arc::new(PlMutex::new(NativeContractsCache::default())),
            None,
        )
    })
    .expect("engine");

    NativeContractLookup::install_provider(Arc::new(EmptyProvider));

    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load entry script");

    let err = engine
        .call_contract_dynamic(
            &target_hash,
            "balanceOf",
            CallFlags::READ_STATES | CallFlags::ALLOW_CALL,
            vec![StackItem::from_byte_string(UInt160::zero().to_bytes())],
        )
        .expect_err("captured policy provider must reject the blocked target");

    assert!(
        err.to_string()
            .contains(&format!("The contract {target_hash} has been blocked.")),
        "unexpected error: {err}"
    );
}

#[test]
fn call_contract_internal_checks_policy_before_return_type_mismatch() {
    let _provider_guard = lock_provider();

    let target_hash =
        UInt160::parse("0xb1b2c3d4e5f60718293a4b5c6d7e8f0102030407").expect("target hash");
    install_blocking_policy(target_hash);

    let contract = build_mock_contract(target_hash);
    let method = contract
        .manifest
        .abi
        .get_method_ref("balanceOf", 1)
        .cloned()
        .expect("balanceOf method");
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        HashMap::new(),
        Arc::new(PlMutex::new(NativeContractsCache::default())),
        None,
    )
    .expect("engine");

    let result = engine.call_contract_internal(
        &contract,
        &method,
        CallFlags::ALL,
        false,
        &[StackItem::from_byte_string(UInt160::zero().to_bytes())],
    );
    let err = match result {
        Ok(_) => panic!("blocked target must be rejected before return type validation"),
        Err(err) => err,
    };

    assert!(
        err.to_string()
            .contains(&format!("The contract {target_hash} has been blocked.")),
        "unexpected error: {err}"
    );
}

#[test]
fn call_contract_dynamic_faults_when_policy_provider_is_missing() {
    let _provider_guard = lock_provider();
    NativeContractLookup::install_provider(Arc::new(EmptyProvider));

    let target_hash =
        UInt160::parse("0xb1b2c3d4e5f60718293a4b5c6d7e8f0102030406").expect("target hash");
    let mut contracts: HashMap<UInt160, ContractState> = HashMap::new();
    contracts.insert(target_hash, build_mock_contract(target_hash));
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        contracts,
        Arc::new(PlMutex::new(NativeContractsCache::default())),
        None,
    )
    .expect("engine");

    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load entry script");

    let err = engine
        .call_contract_dynamic(
            &target_hash,
            "balanceOf",
            CallFlags::READ_STATES | CallFlags::ALLOW_CALL,
            vec![StackItem::from_byte_string(UInt160::zero().to_bytes())],
        )
        .expect_err("missing Policy native contract must not fail open");

    assert!(
        err.to_string().contains("PolicyContract"),
        "unexpected error: {err}"
    );
    assert_eq!(
        engine.invocation_stack().len(),
        1,
        "target contract must not be loaded when the policy gate cannot run"
    );
}

/// Builds a synthetic contract whose single method executes `script` from
/// offset 0. Used by the `call_from_native_contract_returning` tests.
fn build_returning_mock(
    hash: UInt160,
    method_name: &str,
    return_type: ContractParameterType,
    script: Vec<u8>,
) -> ContractState {
    let nef = NefFile::new("test".to_string(), script);
    let method =
        ContractMethodDescriptor::new(method_name.to_string(), Vec::new(), return_type, 0, false)
            .expect("descriptor");
    let abi = ContractAbi::new(vec![method], Vec::new());
    let manifest = ContractManifest {
        name: "ReturningMock".to_string(),
        groups: Vec::new(),
        features: std::collections::HashMap::new(),
        supported_standards: Vec::new(),
        abi,
        permissions: vec![ContractPermission::default_wildcard()],
        trusts: WildCardContainer::default(),
        extra: None,
    };
    ContractState::new(2, hash, nef, manifest)
}

/// Builds an engine preloaded with `contracts` and an entry context (a bare
/// RET script) standing in for the native frame the primitive is called
/// from.
fn engine_with_entry(contracts: HashMap<UInt160, ContractState>) -> ApplicationEngine {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        contracts,
        Arc::new(PlMutex::new(NativeContractsCache::default())),
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load entry script");
    engine
}

/// The returning call yields the callee's result and the callee observes
/// the supplied calling script hash (C# `NativeCallingScriptHash`): the
/// callee script returns `System.Runtime.GetCallingScriptHash`.
#[test]
fn returning_call_yields_result_and_native_calling_hash() {
    let _provider_guard = lock_provider();
    install_allowing_policy();

    let target_hash = UInt160::from_bytes(&[0xCD; 20]).expect("hash");
    let calling_hash = UInt160::from_bytes(&[0xAB; 20]).expect("hash");

    let mut script = vec![OpCode::SYSCALL.byte()];
    script.extend_from_slice(
        &neo_vm_rs::interop_hash("System.Runtime.GetCallingScriptHash").to_le_bytes(),
    );
    script.push(OpCode::RET.byte());

    let mut contracts = HashMap::new();
    contracts.insert(
        target_hash,
        build_returning_mock(
            target_hash,
            "whoCalls",
            ContractParameterType::Hash160,
            script,
        ),
    );
    let mut engine = engine_with_entry(contracts);

    let result = engine
        .call_from_native_contract_returning(&calling_hash, &target_hash, "whoCalls", vec![])
        .expect("returning call succeeds");

    assert_eq!(
        result.as_bytes().expect("hash bytes"),
        calling_hash.to_bytes()
    );
    // The invocation stack is back at the native frame and nothing faulted.
    assert_eq!(engine.invocation_stack().len(), 1);
    assert_ne!(engine.state(), VMState::FAULT);
    // The result was consumed from the native frame's evaluation stack.
    assert_eq!(
        engine
            .current_context()
            .expect("entry context")
            .evaluation_stack()
            .len(),
        0
    );
}

#[test]
fn queued_native_calls_run_in_enqueue_order() {
    let _provider_guard = lock_provider();
    install_allowing_policy();

    let calling_hash = UInt160::from_bytes(&[0xAB; 20]).expect("hash");
    let first_hash = UInt160::from_bytes(&[0xD1; 20]).expect("hash");
    let second_hash = UInt160::from_bytes(&[0xD2; 20]).expect("hash");

    let mut first_script = ScriptBuilder::new();
    first_script.emit_push_string("first");
    first_script
        .emit_syscall("System.Runtime.Log")
        .expect("emit Runtime.Log");
    first_script.emit_opcode(OpCode::RET);

    let mut second_script = ScriptBuilder::new();
    second_script.emit_push_string("second");
    second_script
        .emit_syscall("System.Runtime.Log")
        .expect("emit Runtime.Log");
    second_script.emit_opcode(OpCode::RET);

    let mut contracts = HashMap::new();
    contracts.insert(
        first_hash,
        build_returning_mock(
            first_hash,
            "marker",
            ContractParameterType::Void,
            first_script.to_array(),
        ),
    );
    contracts.insert(
        second_hash,
        build_returning_mock(
            second_hash,
            "marker",
            ContractParameterType::Void,
            second_script.to_array(),
        ),
    );
    let mut engine = engine_with_entry(contracts);

    engine.queue_contract_call_from_native(calling_hash, first_hash, "marker", vec![]);
    engine.queue_contract_call_from_native(calling_hash, second_hash, "marker", vec![]);
    engine
        .process_pending_native_calls()
        .expect("queued calls load");
    let state = engine.execute_allow_fault();

    assert_eq!(state, VMState::HALT, "{:?}", engine.fault_exception());
    let messages = engine
        .logs()
        .iter()
        .map(|event| event.message.as_str())
        .collect::<Vec<_>>();
    assert_eq!(messages, ["first", "second"]);
}

/// C# `CallFromNativeContractAsync<T>` passes `hasReturnValue: true`, so a
/// `Void` callee is rejected with "The return value type does not match."
#[test]
fn returning_call_rejects_void_method() {
    let _provider_guard = lock_provider();
    install_allowing_policy();

    let target_hash = UInt160::from_bytes(&[0xCE; 20]).expect("hash");
    let mut contracts = HashMap::new();
    contracts.insert(
        target_hash,
        build_returning_mock(
            target_hash,
            "voidMethod",
            ContractParameterType::Void,
            vec![OpCode::RET.byte()],
        ),
    );
    let mut engine = engine_with_entry(contracts);

    let err = engine
        .call_from_native_contract_returning(&UInt160::zero(), &target_hash, "voidMethod", vec![])
        .expect_err("void method must be rejected");
    assert!(
        err.to_string().contains("return value type does not match"),
        "unexpected error: {err}"
    );
}

/// C# `CallFromNativeContractAsync` (void overload) passes `hasReturnValue:
/// false`, accepts Void callees, and does not resume the native frame until the
/// callee context has returned.
#[test]
fn void_call_accepts_void_method_and_runs_to_completion() {
    let _provider_guard = lock_provider();
    install_allowing_policy();

    let target_hash = UInt160::from_bytes(&[0xD0; 20]).expect("hash");
    let mut contracts = HashMap::new();
    contracts.insert(
        target_hash,
        build_returning_mock(
            target_hash,
            "accept",
            ContractParameterType::Void,
            vec![OpCode::RET.byte()],
        ),
    );
    let mut engine = engine_with_entry(contracts);

    engine
        .call_from_native_contract_void(&UInt160::zero(), &target_hash, "accept", vec![])
        .expect("void call succeeds");

    assert_eq!(engine.invocation_stack().len(), 1);
    assert_ne!(engine.state(), VMState::FAULT);
    assert_eq!(
        engine
            .current_context()
            .expect("entry context")
            .evaluation_stack()
            .len(),
        0
    );
}

/// C# `CallFromNativeContractAsync<T>` funnels through
/// `CallContractInternal`, so the Policy blocked-contract gate must reject
/// native-to-contract calls before the callee context is loaded.
#[test]
fn returning_call_rejects_policy_blocked_target() {
    let _provider_guard = lock_provider();

    // Share the dynamic-call blocked hash so the global test provider
    // remains compatible when Rust runs these tests in parallel.
    let target_hash =
        UInt160::parse("0xb1b2c3d4e5f60718293a4b5c6d7e8f0102030405").expect("target hash");
    install_blocking_policy(target_hash);

    let mut contracts = HashMap::new();
    contracts.insert(
        target_hash,
        build_returning_mock(
            target_hash,
            "answer",
            ContractParameterType::Integer,
            vec![OpCode::PUSH1.byte(), OpCode::RET.byte()],
        ),
    );
    let mut engine = engine_with_entry(contracts);

    let err = engine
        .call_from_native_contract_returning(&UInt160::zero(), &target_hash, "answer", vec![])
        .expect_err("blocked native call must be rejected");

    assert!(
        err.to_string()
            .contains(&format!("The contract {target_hash} has been blocked.")),
        "unexpected error: {err}"
    );
    assert_eq!(
        engine.invocation_stack().len(),
        1,
        "blocked native call must not load the callee context"
    );
}

/// A callee that throws (and nothing inside the callee catches) faults the
/// whole engine — the primitive surfaces an error and the VM is FAULTed,
/// mirroring C#'s `VMUnhandledException` for `contractTasks` contexts.
#[test]
fn returning_call_propagates_callee_throw_as_engine_fault() {
    let _provider_guard = lock_provider();
    install_allowing_policy();

    let target_hash = UInt160::from_bytes(&[0xCF; 20]).expect("hash");
    let mut contracts = HashMap::new();
    contracts.insert(
        target_hash,
        build_returning_mock(
            target_hash,
            "explode",
            ContractParameterType::Integer,
            vec![OpCode::PUSH1.byte(), OpCode::THROW.byte()],
        ),
    );
    let mut engine = engine_with_entry(contracts);

    let result = engine.call_from_native_contract_returning(
        &UInt160::zero(),
        &target_hash,
        "explode",
        vec![],
    );
    assert!(result.is_err(), "callee throw must surface as an error");
    assert_eq!(engine.state(), VMState::FAULT);
}

/// Hash for the test-only interop the boundary test uses to invoke the
/// primitive from inside a script (standing in for a native method).
const BOUNDARY_TEST_SYSCALL: &str = "Test.NativeCallReturning";
const QUEUED_BOUNDARY_TEST_SYSCALL: &str = "Test.QueueNativeCall";

fn boundary_test_handler(
    app: &mut ApplicationEngine,
    _engine: &mut neo_vm::ExecutionEngine,
) -> neo_vm::VmResult<()> {
    let target_hash = UInt160::from_bytes(&[0xDF; 20]).expect("hash");
    match app.call_from_native_contract_returning(&UInt160::zero(), &target_hash, "explode", vec![])
    {
        Ok(_) => Err(neo_vm::VmError::invalid_operation_msg(
            "boundary test: callee unexpectedly returned",
        )),
        Err(err) => Err(neo_vm::VmError::invalid_operation_msg(err.to_string())),
    }
}

fn queued_boundary_test_handler(
    app: &mut ApplicationEngine,
    _engine: &mut neo_vm::ExecutionEngine,
) -> neo_vm::VmResult<()> {
    let calling_hash = UInt160::from_bytes(&[0xAB; 20]).expect("hash");
    let target_hash = UInt160::from_bytes(&[0xDF; 20]).expect("hash");
    app.queue_contract_call_from_native(calling_hash, target_hash, "explode", vec![]);
    app.process_pending_native_calls()
        .map_err(|err| neo_vm::VmError::invalid_operation_msg(err.to_string()))
}

/// A TRY armed below the native frame cannot catch an exception escaping a
/// returning native call: C# throws `VMUnhandledException` when the
/// registered context unloads, before any lower TRY is consulted. The entry
/// script arms TRY/CATCH around the call; the engine must FAULT (a broken
/// boundary would run the CATCH and HALT with `2` on the result stack).
#[test]
fn returning_call_exception_cannot_be_caught_below_native_frame() {
    let _provider_guard = lock_provider();
    install_allowing_policy();

    let target_hash = UInt160::from_bytes(&[0xDF; 20]).expect("hash");
    let mut contracts = HashMap::new();
    contracts.insert(
        target_hash,
        build_returning_mock(
            target_hash,
            "explode",
            ContractParameterType::Integer,
            vec![OpCode::PUSH1.byte(), OpCode::THROW.byte()],
        ),
    );

    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        contracts,
        Arc::new(PlMutex::new(NativeContractsCache::default())),
        None,
    )
    .expect("engine");
    engine
        .register_host_service(
            BOUNDARY_TEST_SYSCALL,
            0,
            CallFlags::NONE,
            boundary_test_handler,
        )
        .expect("register test interop");

    // ip0: TRY catch=+10 (-> ip10), no finally
    // ip3: SYSCALL Test.NativeCallReturning
    // ip8: ENDTRY +4 (-> ip12)
    // ip10: PUSH2; RET            <- catch handler (must NOT run)
    // ip12: PUSH1; RET
    let mut script = vec![OpCode::TRY.byte(), 10, 0, OpCode::SYSCALL.byte()];
    script.extend_from_slice(&neo_vm_rs::interop_hash(BOUNDARY_TEST_SYSCALL).to_le_bytes());
    script.extend_from_slice(&[
        OpCode::ENDTRY.byte(),
        4,
        OpCode::PUSH2.byte(),
        OpCode::RET.byte(),
        OpCode::PUSH1.byte(),
        OpCode::RET.byte(),
    ]);

    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("load entry script");
    let state = engine.execute_allow_fault();

    assert_eq!(
        state,
        VMState::FAULT,
        "the exception must fault the engine, not reach the CATCH"
    );
    assert_eq!(
        engine.result_stack().len(),
        0,
        "the CATCH handler must not have produced a result"
    );
}

/// The queued native-call path used by NEP-17 `onNEP17Payment` must enforce
/// the same boundary as the synchronous native-call helpers. C# registers
/// `CallFromNativeContractAsync` contexts in `contractTasks`, so a callback
/// exception crossing that native boundary faults the whole engine and cannot
/// be caught by a TRY in the caller below `System.Contract.CallNative`.
#[test]
fn queued_native_call_exception_cannot_be_caught_below_native_frame() {
    let _provider_guard = lock_provider();
    install_allowing_policy();

    let target_hash = UInt160::from_bytes(&[0xDF; 20]).expect("hash");
    let mut contracts = HashMap::new();
    contracts.insert(
        target_hash,
        build_returning_mock(
            target_hash,
            "explode",
            ContractParameterType::Void,
            vec![OpCode::PUSH1.byte(), OpCode::THROW.byte()],
        ),
    );

    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new_with_preloaded_native(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        contracts,
        Arc::new(PlMutex::new(NativeContractsCache::default())),
        None,
    )
    .expect("engine");
    engine
        .register_host_service(
            QUEUED_BOUNDARY_TEST_SYSCALL,
            0,
            CallFlags::NONE,
            queued_boundary_test_handler,
        )
        .expect("register test interop");

    // ip0: TRY catch=+10 (-> ip10), no finally
    // ip3: SYSCALL Test.QueueNativeCall
    // ip8: ENDTRY +4 (-> ip12)
    // ip10: PUSH2; RET            <- catch handler (must NOT run)
    // ip12: PUSH1; RET
    let mut script = vec![OpCode::TRY.byte(), 10, 0, OpCode::SYSCALL.byte()];
    script.extend_from_slice(&neo_vm_rs::interop_hash(QUEUED_BOUNDARY_TEST_SYSCALL).to_le_bytes());
    script.extend_from_slice(&[
        OpCode::ENDTRY.byte(),
        4,
        OpCode::PUSH2.byte(),
        OpCode::RET.byte(),
        OpCode::PUSH1.byte(),
        OpCode::RET.byte(),
    ]);

    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("load entry script");
    let state = engine.execute_allow_fault();

    assert_eq!(
        state,
        VMState::FAULT,
        "the queued callback exception must fault the engine, not reach the CATCH"
    );
    assert_eq!(
        engine.result_stack().len(),
        0,
        "the CATCH handler must not have produced a result"
    );
}
