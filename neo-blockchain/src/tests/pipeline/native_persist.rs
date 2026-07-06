use super::*;
// `invocation_script`/`verification_script` on `Witness` are trait methods.
use neo_execution::native_contract_provider::{NativeProviderTestGuard, lock_native_provider};
use neo_manifest::{ContractManifest, ContractMethodDescriptor, NefFile};
use neo_primitives::Witness as _;
use neo_serialization::BinarySerializer;
use neo_storage::StorageKey;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use std::sync::atomic::{AtomicUsize, Ordering};

/// NEO `Prefix_Committee` (C# NeoToken).
const NEO_PREFIX_COMMITTEE: u8 = 14;
/// NEO `Prefix_VotersCount`.
const NEO_PREFIX_VOTERS_COUNT: u8 = 1;
/// NEO `Prefix_GasPerBlock`.
const NEO_PREFIX_GAS_PER_BLOCK: u8 = 29;
/// NEO `Prefix_RegisterPrice`.
const NEO_PREFIX_REGISTER_PRICE: u8 = 13;
/// Shared NEP-17 `Prefix_Account` / `Prefix_TotalSupply`.
const NEP17_PREFIX_ACCOUNT: u8 = 20;
const NEP17_PREFIX_TOTAL_SUPPLY: u8 = 11;
/// Oracle `Prefix_Price` / `Prefix_RequestId`.
const ORACLE_PREFIX_PRICE: u8 = 5;
const ORACLE_PREFIX_REQUEST_ID: u8 = 9;

fn lock_provider() -> NativeProviderTestGuard {
    lock_native_provider()
}

fn standard_resources() -> NativePersistResources {
    NativePersistResources::from_provider(Arc::new(
        neo_native_contracts::StandardNativeProvider::new(),
    ))
}

fn persist_with_resources(
    snapshot: Arc<DataCache>,
    block: Arc<Block>,
    settings: &ProtocolSettings,
    resources: &NativePersistResources,
) -> CoreResult<NativePersistOutcome> {
    persist_block_natives_with_resources(
        snapshot,
        block,
        settings,
        NativePersistOptions::default(),
        resources,
    )
}

struct CountingNativeProvider {
    inner: neo_native_contracts::StandardNativeProvider,
    all_contracts_calls: Arc<AtomicUsize>,
}

impl CountingNativeProvider {
    fn new(all_contracts_calls: Arc<AtomicUsize>) -> Self {
        Self {
            inner: neo_native_contracts::StandardNativeProvider::new(),
            all_contracts_calls,
        }
    }
}

impl neo_execution::native_contract_provider::NativeContractProvider for CountingNativeProvider {
    fn get_native_contract(
        &self,
        hash: &UInt160,
    ) -> Option<Arc<dyn neo_execution::NativeContract>> {
        self.inner.get_native_contract(hash)
    }

    fn get_native_contract_by_name(
        &self,
        name: &str,
    ) -> Option<Arc<dyn neo_execution::NativeContract>> {
        self.inner.get_native_contract_by_name(name)
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn neo_execution::NativeContract>> {
        self.all_contracts_calls.fetch_add(1, Ordering::Relaxed);
        self.inner.all_native_contracts()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        self.inner.all_native_contract_hashes()
    }

    fn current_block_index(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.inner.current_block_index(snapshot)
    }
}

struct EmptyNativeProvider;

impl neo_execution::native_contract_provider::NativeContractProvider for EmptyNativeProvider {
    fn get_native_contract(
        &self,
        _hash: &UInt160,
    ) -> Option<Arc<dyn neo_execution::NativeContract>> {
        None
    }

    fn get_native_contract_by_name(
        &self,
        _name: &str,
    ) -> Option<Arc<dyn neo_execution::NativeContract>> {
        None
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn neo_execution::NativeContract>> {
        Vec::new()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        Vec::new()
    }
}

fn neo_id() -> i32 {
    neo_native_contracts::NeoToken::ID
}

fn get(snapshot: &DataCache, id: i32, key: Vec<u8>) -> Option<Vec<u8>> {
    snapshot
        .get(&StorageKey::new(id, key))
        .map(|item| item.value_bytes().into_owned())
}

fn fund_gas(snapshot: &DataCache, account: &UInt160, amount: i64) {
    let mut gas_key = vec![NEP17_PREFIX_ACCOUNT];
    gas_key.extend_from_slice(&account.to_bytes());
    let account_state = StackItem::from_struct(vec![StackItem::from_int(BigInt::from(amount))]);
    let account_bytes =
        BinarySerializer::serialize(&account_state, &ExecutionEngineLimits::default()).unwrap();
    snapshot.add(
        StorageKey::new(neo_native_contracts::GasToken::ID, gas_key),
        neo_storage::StorageItem::from_bytes(account_bytes),
    );
}

fn deploy_contract(snapshot: &DataCache, state: &neo_execution::ContractState) {
    let mut key = vec![0x08];
    key.extend_from_slice(&state.hash.to_bytes());
    snapshot.add(
        StorageKey::new(neo_native_contracts::ContractManagement::ID, key),
        neo_storage::StorageItem::from_bytes(state.serialize_contract_record().unwrap()),
    );
}

fn throwing_nep17_receiver_contract(hash: UInt160) -> neo_execution::ContractState {
    let nef = NefFile::new(
        "throwing-nep17-receiver".to_string(),
        vec![
            neo_vm_rs::OpCode::PUSH1.byte(),
            neo_vm_rs::OpCode::THROW.byte(),
        ],
    );
    let mut manifest = ContractManifest::new("ThrowingNep17Receiver".to_string());
    manifest.abi.methods.push(
        ContractMethodDescriptor::new(
            "onNEP17Payment".to_string(),
            vec![
                neo_manifest::ContractParameterDefinition::new(
                    "from".to_string(),
                    neo_primitives::ContractParameterType::Hash160,
                )
                .unwrap(),
                neo_manifest::ContractParameterDefinition::new(
                    "amount".to_string(),
                    neo_primitives::ContractParameterType::Integer,
                )
                .unwrap(),
                neo_manifest::ContractParameterDefinition::new(
                    "data".to_string(),
                    neo_primitives::ContractParameterType::Any,
                )
                .unwrap(),
            ],
            neo_primitives::ContractParameterType::Void,
            0,
            false,
        )
        .expect("method descriptor"),
    );
    neo_execution::ContractState::new(7, hash, nef, manifest)
}

fn policy_get_exec_fee_factor_script() -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(0);
    builder.emit_pack();
    builder.emit_push_int(i64::from(neo_manifest::CallFlags::READ_STATES.bits()));
    builder.emit_push_string("getExecFeeFactor");
    builder.emit_push(&neo_native_contracts::PolicyContract::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");
    builder.to_array()
}

fn gas_transfer_script(from: &UInt160, to: &UInt160, amount: i64) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_opcode(neo_vm_rs::OpCode::PUSHNULL);
    builder.emit_push_int(amount);
    builder.emit_push(&to.to_array());
    builder.emit_push(&from.to_array());
    builder.emit_push_int(4);
    builder.emit_pack();
    builder.emit_push_int(i64::from(neo_manifest::CallFlags::ALL.bits()));
    builder.emit_push_string("transfer");
    builder.emit_push(&neo_native_contracts::GasToken::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");
    builder.to_array()
}

fn signed_test_tx(sender: UInt160, nonce: u32, script: Vec<u8>) -> neo_payloads::Transaction {
    let mut tx = neo_payloads::Transaction::new();
    tx.set_nonce(nonce);
    tx.set_script(script);
    tx.set_system_fee(1_0000_0000);
    tx.set_signers(vec![neo_payloads::Signer::new(
        sender,
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    tx
}

#[test]
fn trace_tx_filter_is_disabled_when_env_is_absent() {
    let tx_hash = UInt256::from([0x11; 32]);

    let filter = TraceTxFilter::from_raw(None);

    assert!(!filter.matches(&tx_hash));
}

#[test]
fn trace_tx_filter_matches_wildcards_and_listed_hashes() {
    let tx_hash = UInt256::from([0x22; 32]);
    let other_hash = UInt256::from([0x33; 32]);

    let raw = format!(" {},not-a-match ", tx_hash);
    let filter = TraceTxFilter::from_raw(Some(&raw));

    assert!(filter.matches(&tx_hash));
    assert!(!filter.matches(&other_hash));
    assert!(TraceTxFilter::from_raw(Some(" all ")).matches(&other_hash));
    assert!(TraceTxFilter::from_raw(Some("*")).matches(&other_hash));
}

#[test]
fn trace_tx_filter_default_path_returns_before_hash_formatting() {
    let source = include_str!("../../pipeline/native_persist.rs");
    let matcher = source
        .split("fn matches(&self, tx_hash: &UInt256) -> bool")
        .nth(1)
        .and_then(|tail| tail.split("fn trace_tx_frames").next())
        .expect("TraceTxFilter::matches source");
    let empty_guard = matcher
        .find("self.hashes.is_empty()")
        .expect("default no-trace guard should avoid hash formatting");
    let hash_format = matcher
        .find("tx_hash.to_string()")
        .expect("listed trace hashes still need string matching");

    assert!(
        empty_guard < hash_format,
        "default no-trace path should return before formatting tx hash"
    );
}

#[test]
fn genesis_block_matches_csharp_create_genesis_block() {
    let settings = ProtocolSettings::default();
    let block = genesis_block(&settings).expect("genesis block");
    assert_eq!(block.index(), 0);
    assert_eq!(block.header.version(), 0);
    assert_eq!(*block.header.prev_hash(), UInt256::zero());
    assert_eq!(*block.header.merkle_root(), UInt256::zero());
    assert_eq!(block.header.timestamp(), 1_468_595_301_000);
    assert_eq!(block.header.nonce(), 2_083_236_893);
    assert_eq!(block.header.primary_index(), 0);
    assert!(block.transactions.is_empty());
    // NextConsensus = BFT address (m = n - (n-1)/3) of the standby validators.
    let validators = settings.standby_validators();
    let m = validators.len() - (validators.len() - 1) / 3;
    let script =
        neo_vm::script_builder::redeem_script::RedeemScript::multi_sig_redeem_script_from_points(
            m,
            &validators,
        )
        .unwrap();
    assert_eq!(
        *block.header.next_consensus(),
        UInt160::from_script(&script)
    );
    // Witness: empty invocation, PUSH1 verification.
    assert!(block.header.witness.invocation_script().is_empty());
    assert_eq!(
        block.header.witness.verification_script(),
        &[neo_vm_rs::OpCode::PUSH1.byte()]
    );
}

#[test]
fn genesis_persist_seeds_native_state_and_mints() {
    let resources = standard_resources();
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    let block = Arc::new(genesis_block(&settings).expect("genesis block"));

    let outcome = persist_with_resources(Arc::clone(&snapshot), block, &settings, &resources)
        .expect("genesis persist");

    // Genesis-active natives initialized (NeoToken + OracleContract among them).
    assert!(outcome.initialized.iter().any(|n| n == "NeoToken"));
    assert!(outcome.initialized.iter().any(|n| n == "OracleContract"));
    // C# allApplicationExecuted for an empty block: the OnPersist
    // engine and the PostPersist engine.
    assert_eq!(outcome.application_executed.len(), 2);
    assert_eq!(
        outcome.application_executed[0].trigger,
        neo_primitives::TriggerType::OnPersist
    );
    assert_eq!(
        outcome.application_executed[1].trigger,
        neo_primitives::TriggerType::PostPersist
    );

    // --- NeoToken.Initialize seeds (byte-exact) ---
    // Committee cache: Array of Struct[pubkey, 0] in standby order.
    let expected_committee = StackItem::from_array(
        settings
            .standby_committee
            .iter()
            .map(|p| {
                StackItem::from_struct(vec![
                    StackItem::from_byte_string(p.to_bytes()),
                    StackItem::from_int(BigInt::from(0)),
                ])
            })
            .collect::<Vec<_>>(),
    );
    let expected_committee_bytes =
        BinarySerializer::serialize(&expected_committee, &ExecutionEngineLimits::default())
            .unwrap();
    assert_eq!(
        get(&snapshot, neo_id(), vec![NEO_PREFIX_COMMITTEE]),
        Some(expected_committee_bytes)
    );
    // Voters count: BigInteger zero = empty bytes.
    assert_eq!(
        get(&snapshot, neo_id(), vec![NEO_PREFIX_VOTERS_COUNT]),
        Some(Vec::new())
    );
    // gasPerBlock record at big-endian index 0 = 5 GAS.
    let mut gpb_key = vec![NEO_PREFIX_GAS_PER_BLOCK];
    gpb_key.extend_from_slice(&0u32.to_be_bytes());
    assert_eq!(
        get(&snapshot, neo_id(), gpb_key),
        Some(BigInt::from(500_000_000i64).to_signed_bytes_le())
    );
    // registerPrice = 1000 GAS.
    assert_eq!(
        get(&snapshot, neo_id(), vec![NEO_PREFIX_REGISTER_PRICE]),
        Some(BigInt::from(100_000_000_000i64).to_signed_bytes_le())
    );

    // --- PolicyContract.Initialize seeds (byte-exact) ---
    // Policy is genesis-active (ActiveIn == null) and its Initialize writes
    // FeePerByte=1000, ExecFeeFactor=30, StoragePrice=100000 at block 0
    // (PolicyContract.cs:141-143). These MUST be committed by genesis persist,
    // otherwise getExecFeeFactor reads empty storage and returns 0 (the
    // v3.10.0 consistency testnet failure: Policy_getExecFeeFactor).
    const POLICY_PREFIX_FEE_PER_BYTE: u8 = 10;
    const POLICY_PREFIX_EXEC_FEE_FACTOR: u8 = 18;
    const POLICY_PREFIX_STORAGE_PRICE: u8 = 19;
    let policy_id = neo_native_contracts::PolicyContract::ID;
    assert_eq!(
        get(&snapshot, policy_id, vec![POLICY_PREFIX_FEE_PER_BYTE]),
        Some(BigInt::from(1000i64).to_signed_bytes_le()),
        "Policy FeePerByte must be initialized at genesis"
    );
    assert_eq!(
        get(&snapshot, policy_id, vec![POLICY_PREFIX_EXEC_FEE_FACTOR]),
        Some(BigInt::from(30i64).to_signed_bytes_le()),
        "Policy ExecFeeFactor must be initialized at genesis (v3.10.0 parity)"
    );
    assert_eq!(
        get(&snapshot, policy_id, vec![POLICY_PREFIX_STORAGE_PRICE]),
        Some(BigInt::from(100_000i64).to_signed_bytes_le()),
        "Policy StoragePrice must be initialized at genesis"
    );

    // --- The genesis NEO mint: 100M NEO to the standby-validator BFT address ---
    let bft = bft_address(&settings.standby_validators()).unwrap();
    let mut account_key = vec![NEP17_PREFIX_ACCOUNT];
    account_key.extend_from_slice(&bft.to_bytes());
    let expected_account = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(100_000_000)),
        StackItem::from_int(BigInt::from(0)),
        StackItem::null(),
        StackItem::from_int(BigInt::from(0)),
    ]);
    let expected_account_bytes =
        BinarySerializer::serialize(&expected_account, &ExecutionEngineLimits::default()).unwrap();
    assert_eq!(
        get(&snapshot, neo_id(), account_key),
        Some(expected_account_bytes)
    );
    assert_eq!(
        get(&snapshot, neo_id(), vec![NEP17_PREFIX_TOTAL_SUPPLY]),
        Some(BigInt::from(100_000_000).to_signed_bytes_le())
    );
    // The mint's Transfer(null, bft, 100M) notification was emitted by NEO.
    let transfer = outcome
        .on_persist_notifications
        .iter()
        .find(|n| n.event_name == "Transfer")
        .expect("genesis NEO Transfer notification");
    assert_eq!(
        transfer.script_hash,
        neo_native_contracts::NeoToken::script_hash(),
        "the genesis mint Transfer is emitted by the NEO contract"
    );
    assert!(
        matches!(transfer.state[0], StackItem::Null),
        "from = null (mint)"
    );
    assert_eq!(
        transfer.state[1].as_bytes().expect("to address bytes"),
        bft.to_bytes(),
        "to = the standby-validator BFT address"
    );
    assert_eq!(
        transfer.state[2].as_int().expect("amount"),
        BigInt::from(100_000_000),
        "amount = the full NEO TotalAmount"
    );
    let first_on_persist_notification = outcome
        .on_persist_notifications
        .first()
        .expect("genesis OnPersist emits native deploy notifications");
    assert_eq!(
        first_on_persist_notification.event_name, "Deploy",
        "C# ContractManagement.OnPersist deploys genesis natives before NEO/GAS initialize Transfer events"
    );
    assert_eq!(
        first_on_persist_notification.script_hash,
        neo_native_contracts::ContractManagement::script_hash(),
        "Deploy is emitted by ContractManagement"
    );
    assert_eq!(
        first_on_persist_notification.state[0]
            .as_bytes()
            .expect("deployed native hash"),
        neo_native_contracts::ContractManagement::script_hash().to_bytes(),
        "the first genesis deployment is ContractManagement itself"
    );
    // No CommitteeChanged at genesis: the recomputed committee equals the
    // seeded standby committee.
    assert!(
        !outcome
            .on_persist_notifications
            .iter()
            .any(|n| n.event_name == "CommitteeChanged"),
        "genesis recompute must not change the committee"
    );

    // --- OracleContract.Initialize seeds ---
    let oracle_id = neo_native_contracts::OracleContract::ID;
    assert_eq!(
        get(&snapshot, oracle_id, vec![ORACLE_PREFIX_REQUEST_ID]),
        Some(Vec::new()),
        "RequestId seeds as BigInteger.Zero (empty bytes)"
    );
    assert_eq!(
        get(&snapshot, oracle_id, vec![ORACLE_PREFIX_PRICE]),
        Some(BigInt::from(50_000_000i64).to_signed_bytes_le()),
        "oracle price seeds as 0.5 GAS"
    );

    // --- NeoToken.PostPersist: committee reward minted at genesis ---
    // gasPerBlock(5 GAS) * CommitteeRewardRatio(10) / 100 = 0.5 GAS to the
    // signature address of committee[0 % m] = standby_committee[0].
    let member = &settings.standby_committee[0];
    let script = neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
        &member.to_bytes(),
    );
    let reward_account = UInt160::from_script(&script);
    let mut gas_key = vec![NEP17_PREFIX_ACCOUNT];
    gas_key.extend_from_slice(&reward_account.to_bytes());
    let gas_account = get(&snapshot, neo_native_contracts::GasToken::ID, gas_key)
        .expect("committee reward GAS account");
    let decoded =
        BinarySerializer::deserialize(&gas_account, &ExecutionEngineLimits::default(), None)
            .unwrap();
    let StackItem::Struct(fields) = decoded else {
        panic!("GAS account is not a struct");
    };
    assert_eq!(
        fields.items().first().unwrap().as_int().unwrap(),
        BigInt::from(50_000_000i64),
        "committee member 0 earns 0.5 GAS at genesis"
    );
    let gas_transfer_minted = outcome
        .post_persist_notifications
        .iter()
        .any(|n| n.event_name == "Transfer");
    assert!(gas_transfer_minted, "PostPersist GAS mint emits Transfer");
}

#[test]
fn non_refresh_block_mints_to_rotating_member_without_recompute() {
    let resources = standard_resources();
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    // Persist genesis first so the committee cache + gas records exist.
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_with_resources(Arc::clone(&snapshot), genesis, &settings, &resources)
        .expect("genesis persist");

    // Block 1: not a refresh block for the 21-member committee.
    let mut header = Header::new();
    header.set_index(1);
    let block = Arc::new(Block::from_parts(header, Vec::new()));
    let outcome = persist_with_resources(Arc::clone(&snapshot), block, &settings, &resources)
        .expect("block 1 persist");
    assert!(
        outcome.initialized.is_empty(),
        "no native initializes after genesis"
    );

    // committee[1 % 21] = standby_committee[1] earns 0.5 GAS.
    let member = &settings.standby_committee[1];
    let script = neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
        &member.to_bytes(),
    );
    let reward_account = UInt160::from_script(&script);
    let mut gas_key = vec![NEP17_PREFIX_ACCOUNT];
    gas_key.extend_from_slice(&reward_account.to_bytes());
    let gas_account = get(&snapshot, neo_native_contracts::GasToken::ID, gas_key)
        .expect("committee reward GAS account for member 1");
    let decoded =
        BinarySerializer::deserialize(&gas_account, &ExecutionEngineLimits::default(), None)
            .unwrap();
    let StackItem::Struct(fields) = decoded else {
        panic!("GAS account is not a struct");
    };
    assert_eq!(
        fields.items().first().unwrap().as_int().unwrap(),
        BigInt::from(50_000_000i64)
    );
}

#[test]
fn probe_constants_pin_the_real_native_ids() {
    // The probe hardcodes protocol constants because the blockchain
    // crate only reaches natives through the type-erased provider;
    // pin them against the canonical definitions.
    assert_eq!(LEDGER_CONTRACT_ID, neo_native_contracts::LedgerContract::ID);
    assert_eq!(NEO_TOKEN_ID, neo_native_contracts::NeoToken::ID);
    assert_eq!(NEO_PREFIX_COMMITTEE_KEY, NEO_PREFIX_COMMITTEE);
}

#[test]
fn chain_state_initialized_flips_after_genesis_persist() {
    let resources = standard_resources();
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    assert!(
        !chain_state_initialized(&snapshot),
        "fresh store is uninitialized"
    );

    let block = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_with_resources(Arc::clone(&snapshot), block, &settings, &resources)
        .expect("genesis persist");
    assert!(
        chain_state_initialized(&snapshot),
        "genesis persist initializes the chain"
    );

    // The C#-faithful leg of the probe: a LedgerContract Prefix_Block
    // record alone also reports initialized.
    let ledger_only = DataCache::new(false);
    let mut key = vec![LEDGER_PREFIX_BLOCK];
    key.extend_from_slice(&[0u8; 32]);
    ledger_only.add(
        StorageKey::new(LEDGER_CONTRACT_ID, key),
        neo_storage::StorageItem::from_bytes(vec![1]),
    );
    assert!(chain_state_initialized(&ledger_only));
}

/// Mainnet genesis-hash pin. Oracle:
/// `neo_csharp/tests/Neo.UnitTests/SmartContract/UT_InteropService.cs:872`
/// (`TestGetBlockHash`) asserts block 0's hash under
/// `TestProtocolSettings.Default`, whose `StandbyCommittee` /
/// `ValidatorsCount` are byte-identical to
/// `neo_csharp/src/Neo.CLI/config.mainnet.json` (verified 2026-06-10).
/// The header hash covers only the serialized unsigned header
/// (`Neo.Network.P2P.Helper.CalculateHash` — single SHA-256, no
/// network magic), so the test-chain genesis hash IS the mainnet
/// genesis hash. This transitively pins `NextConsensus`, the
/// standby-validator multisig redeem script, and hash160.
#[test]
fn mainnet_genesis_hash_matches_csharp() {
    let settings = ProtocolSettings::default();
    let block = genesis_block(&settings).expect("genesis block");
    let hash = block.header.try_hash().expect("genesis hash");
    assert_eq!(
        hash.to_string(),
        "0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15",
        "mainnet genesis hash must match the C# oracle \
         (UT_InteropService.TestGetBlockHash)"
    );
}

/// The transaction stage of `Blockchain.Persist`: a HALTing and a
/// FAULTing transaction in one block both execute and get ledger
/// records carrying their final VM state, and the
/// `ApplicationExecuted` list has the C# shape (OnPersist, one per
/// tx, PostPersist).
#[test]
fn persist_executes_transactions_and_records_vm_states() {
    let resources = standard_resources();
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_with_resources(Arc::clone(&snapshot), genesis, &settings, &resources)
        .expect("genesis persist");

    // Fund the fee-paying signer first: C# GasToken.OnPersist burns
    // each transaction's system+network fee from its sender, so a
    // block whose sender holds no GAS faults the OnPersist engine.
    let signer_account = neo_primitives::UInt160::from_bytes(&[0x33; 20]).unwrap();
    let mut gas_key = vec![NEP17_PREFIX_ACCOUNT];
    gas_key.extend_from_slice(&signer_account.to_bytes());
    let account_state =
        StackItem::from_struct(vec![StackItem::from_int(BigInt::from(10_0000_0000i64))]);
    let account_bytes =
        BinarySerializer::serialize(&account_state, &ExecutionEngineLimits::default()).unwrap();
    snapshot.add(
        StorageKey::new(neo_native_contracts::GasToken::ID, gas_key),
        neo_storage::StorageItem::from_bytes(account_bytes),
    );

    // tx1 faults (ABORT), tx2 halts (PUSH1).
    let signer = neo_payloads::Signer::new(signer_account, neo_primitives::WitnessScope::NONE);
    let mut tx1 = neo_payloads::Transaction::new();
    tx1.set_nonce(1);
    tx1.set_script(vec![neo_vm_rs::OpCode::ABORT.byte()]);
    tx1.set_system_fee(1_0000_0000);
    tx1.set_signers(vec![signer.clone()]);
    tx1.set_witnesses(vec![neo_payloads::Witness::empty()]);
    let mut tx2 = neo_payloads::Transaction::new();
    tx2.set_nonce(2);
    tx2.set_script(vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    tx2.set_system_fee(1_0000_0000);
    tx2.set_signers(vec![signer]);
    tx2.set_witnesses(vec![neo_payloads::Witness::empty()]);
    let tx1_hash = tx1.try_hash().unwrap();
    let tx2_hash = tx2.try_hash().unwrap();

    let mut header = Header::new();
    header.set_index(1);
    let block = Arc::new(Block::from_parts(header, vec![tx1, tx2]));
    let block_hash = block.header.try_hash().unwrap();
    let outcome = persist_with_resources(Arc::clone(&snapshot), block, &settings, &resources)
        .expect("block 1 persist");

    // C# allApplicationExecuted: OnPersist, tx1, tx2, PostPersist.
    assert_eq!(outcome.application_executed.len(), 4);
    let tx1_exec = &outcome.application_executed[1];
    assert_eq!(tx1_exec.trigger, neo_primitives::TriggerType::Application);
    assert_eq!(tx1_exec.vm_state, neo_vm_rs::VmState::FAULT);
    assert!(tx1_exec.transaction.is_some());
    let tx2_exec = &outcome.application_executed[2];
    assert_eq!(tx2_exec.vm_state, neo_vm_rs::VmState::HALT);
    // PUSH1 leaves the integer 1 on the result stack.
    assert_eq!(tx2_exec.stack.len(), 1);
    let actual = match &tx2_exec.stack[0] {
        StackValue::Integer(value) => BigInt::from(*value),
        StackValue::BigInteger(bytes) => BigInt::from_signed_bytes_le(bytes),
        item => panic!("expected integer stack value, got {item:?}"),
    };
    assert_eq!(actual, BigInt::from(1));

    // Ledger records carry the final VM states (C# mutates the
    // TransactionState stored by Ledger.OnPersist) and the block
    // records exist.
    let ledger = neo_native_contracts::LedgerContract::new();
    let s1 = ledger
        .get_transaction_state(&snapshot, &tx1_hash)
        .unwrap()
        .expect("tx1 record");
    assert_eq!(s1.state, neo_vm_rs::VmState::FAULT);
    assert_eq!(s1.block_index, 1);
    let s2 = ledger
        .get_transaction_state(&snapshot, &tx2_hash)
        .unwrap()
        .expect("tx2 record");
    assert_eq!(s2.state, neo_vm_rs::VmState::HALT);
    assert_eq!(
        ledger.get_block_hash(&snapshot, 1).unwrap(),
        Some(block_hash)
    );
    let trimmed = ledger
        .get_trimmed_block(&snapshot, &block_hash)
        .unwrap()
        .expect("trimmed block");
    assert_eq!(trimmed.hashes, vec![tx1_hash, tx2_hash]);
    // PostPersist current-block pointer.
    assert_eq!(ledger.current_index(&snapshot).unwrap(), 1);
    assert_eq!(ledger.current_hash(&snapshot).unwrap(), block_hash);
}

#[test]
fn persist_records_fault_when_nep17_receiver_callback_faults() {
    let resources = standard_resources();
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_with_resources(Arc::clone(&snapshot), genesis, &settings, &resources)
        .expect("genesis persist");

    let signer_account = neo_primitives::UInt160::from_bytes(&[0x55; 20]).unwrap();
    let receiver = neo_primitives::UInt160::from_bytes(&[0x66; 20]).unwrap();
    fund_gas(&snapshot, &signer_account, 10_0000_0000);
    deploy_contract(&snapshot, &throwing_nep17_receiver_contract(receiver));

    let script = gas_transfer_script(&signer_account, &receiver, 10_0000);
    let mut tx = signed_test_tx(signer_account, 55, script);
    tx.set_signers(vec![neo_payloads::Signer::new(
        signer_account,
        neo_primitives::WitnessScope::GLOBAL,
    )]);
    let tx_hash = tx.try_hash().unwrap();

    let mut header = Header::new();
    header.set_index(1);
    let block = Arc::new(Block::from_parts(header, vec![tx]));
    let outcome = persist_with_resources(Arc::clone(&snapshot), block, &settings, &resources)
        .expect("block 1 persist");

    assert_eq!(outcome.application_executed.len(), 3);
    let tx_exec = &outcome.application_executed[1];
    assert_eq!(tx_exec.trigger, neo_primitives::TriggerType::Application);
    assert_eq!(
        tx_exec.vm_state,
        neo_vm_rs::VmState::FAULT,
        "a receiver onNEP17Payment exception must fault the transaction"
    );

    let ledger = neo_native_contracts::LedgerContract::new();
    let state = ledger
        .get_transaction_state(&snapshot, &tx_hash)
        .unwrap()
        .expect("tx record");
    assert_eq!(
        state.state,
        neo_vm_rs::VmState::FAULT,
        "persisted ledger TransactionState must carry the callback-induced fault"
    );
}

#[test]
fn bulk_sync_native_persist_skips_replay_artifacts_but_keeps_vm_state() {
    let resources = standard_resources();
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    persist_with_resources(Arc::clone(&snapshot), genesis, &settings, &resources)
        .expect("genesis persist");

    let signer_account = neo_primitives::UInt160::from_bytes(&[0x44; 20]).unwrap();
    let mut gas_key = vec![NEP17_PREFIX_ACCOUNT];
    gas_key.extend_from_slice(&signer_account.to_bytes());
    let account_state =
        StackItem::from_struct(vec![StackItem::from_int(BigInt::from(10_0000_0000i64))]);
    let account_bytes =
        BinarySerializer::serialize(&account_state, &ExecutionEngineLimits::default()).unwrap();
    snapshot.add(
        StorageKey::new(neo_native_contracts::GasToken::ID, gas_key),
        neo_storage::StorageItem::from_bytes(account_bytes),
    );

    let mut tx = neo_payloads::Transaction::new();
    tx.set_nonce(44);
    tx.set_script(vec![neo_vm_rs::OpCode::PUSH1.byte()]);
    tx.set_system_fee(1_0000_0000);
    tx.set_signers(vec![neo_payloads::Signer::new(
        signer_account,
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    let tx_hash = tx.try_hash().unwrap();

    let mut header = Header::new();
    header.set_index(1);
    let block = Arc::new(Block::from_parts(header, vec![tx]));
    let staged = stage_block_natives_with_resources(
        Arc::clone(&snapshot),
        block,
        &settings,
        NativePersistOptions {
            capture_replay_artifacts: false,
        },
        &resources,
    )
    .expect("bulk-sync block stages");

    assert!(
        staged.outcome.application_executed.is_empty(),
        "bulk sync should not materialize ApplicationExecuted replay payloads"
    );
    assert!(
        staged.outcome.on_persist_notifications.is_empty(),
        "bulk sync should not clone OnPersist notification artifacts"
    );
    assert!(
        staged.outcome.post_persist_notifications.is_empty(),
        "bulk sync should not clone PostPersist notification artifacts"
    );
    staged.commit();

    let state = neo_native_contracts::LedgerContract::new()
        .get_transaction_state(&snapshot, &tx_hash)
        .unwrap()
        .expect("tx ledger state remains consensus-visible");
    assert_eq!(state.state, neo_vm_rs::VmState::HALT);
    assert_eq!(state.block_index, 1);
}

#[test]
fn reusable_native_persist_resources_fetch_contract_list_once_for_batch() {
    let _provider_guard = lock_provider();
    let all_contracts_calls = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(CountingNativeProvider::new(Arc::clone(
        &all_contracts_calls,
    )));

    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    let resources = NativePersistResources::from_provider(provider);
    assert_eq!(
        all_contracts_calls.load(Ordering::Relaxed),
        1,
        "resource construction should fetch the canonical contract list once"
    );

    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    let staged = stage_block_natives_with_resources(
        Arc::clone(&snapshot),
        genesis,
        &settings,
        NativePersistOptions {
            capture_replay_artifacts: false,
        },
        &resources,
    )
    .expect("genesis stages with cached resources");
    staged.commit();

    let mut header = Header::new();
    header.set_index(1);
    let block = Arc::new(Block::from_parts(header, Vec::new()));
    let staged = stage_block_natives_with_resources(
        Arc::clone(&snapshot),
        block,
        &settings,
        NativePersistOptions {
            capture_replay_artifacts: false,
        },
        &resources,
    )
    .expect("second block stages with cached resources");
    staged.commit();

    assert_eq!(
        all_contracts_calls.load(Ordering::Relaxed),
        1,
        "bulk persistence should reuse the cached native-contract list"
    );
}

#[test]
fn reusable_native_persist_resources_keep_provider_consistent_after_global_replacement() {
    let _provider_guard = lock_provider();
    let all_contracts_calls = Arc::new(AtomicUsize::new(0));
    let provider = Arc::new(CountingNativeProvider::new(Arc::clone(
        &all_contracts_calls,
    )));

    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    let resources = NativePersistResources::from_provider(provider);

    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    let staged = stage_block_natives_with_resources(
        Arc::clone(&snapshot),
        genesis,
        &settings,
        NativePersistOptions {
            capture_replay_artifacts: false,
        },
        &resources,
    )
    .expect("genesis stages with original provider");
    staged.commit();

    // Simulate a later global provider replacement. The reusable batch resources
    // must stay internally consistent: direct native hooks and engine native
    // lookups should both use the provider captured by the resources.
    neo_execution::native_contract_provider::NativeContractLookup::install_provider(Arc::new(
        EmptyNativeProvider,
    ));

    let signer_account = UInt160::from_bytes(&[0x55; 20]).unwrap();
    fund_gas(&snapshot, &signer_account, 10_0000_0000);
    let tx = signed_test_tx(signer_account, 55, policy_get_exec_fee_factor_script());

    let mut header = Header::new();
    header.set_index(1);
    let block = Arc::new(Block::from_parts(header, vec![tx]));
    let staged = stage_block_natives_with_resources(
        Arc::clone(&snapshot),
        block,
        &settings,
        NativePersistOptions::default(),
        &resources,
    )
    .expect("transaction native lookup should use the batch resource provider");
    let tx_exec = staged
        .outcome
        .application_executed
        .iter()
        .find(|executed| executed.trigger == neo_primitives::TriggerType::Application)
        .expect("transaction application execution");
    assert_eq!(tx_exec.vm_state, neo_vm_rs::VmState::HALT);
    assert_eq!(
        tx_exec.stack,
        vec![StackValue::Integer(30)],
        "Policy.getExecFeeFactor should resolve through the batch resource provider"
    );
}

#[test]
fn native_persist_resources_do_not_install_thread_scoped_provider() {
    let source = include_str!("../../pipeline/native_persist.rs");
    assert!(
        !source.contains("with_scoped_provider"),
        "native persistence resources must pass providers directly into engines, not mutate the thread-scoped global provider"
    );
}

#[test]
fn native_persist_exposes_explicit_resource_commit_path() {
    let source = include_str!("../../pipeline/native_persist.rs");
    assert!(
        source.contains("pub fn persist_block_natives_with_resources"),
        "native persistence should expose a committing path for callers that already own explicit provider resources"
    );
    assert!(
        source.contains("pub fn stage_block_natives_with_resources"),
        "native persistence should expose a staging path for callers that already own explicit provider resources"
    );
    assert!(
        !source.contains("from_installed_provider"),
        "native persistence must not expose installed-provider compatibility constructors"
    );
    assert!(
        !source.contains("NativeContractLookup::native_contract_provider"),
        "native persistence must not read the process-global provider"
    );
}

#[test]
fn reusable_native_persist_resources_cross_echidna_activation_height() {
    let resources = standard_resources();
    let mut settings = ProtocolSettings::default();
    settings
        .hardforks
        .insert(neo_config::Hardfork::HfEchidna, 1);
    let snapshot = Arc::new(DataCache::new(false));

    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    let staged = stage_block_natives_with_resources(
        Arc::clone(&snapshot),
        genesis,
        &settings,
        NativePersistOptions {
            capture_replay_artifacts: false,
        },
        &resources,
    )
    .expect("genesis stages before Echidna");
    staged.commit();
    assert!(
        neo_native_contracts::ContractManagement::get_contract_from_snapshot(
            &snapshot,
            &neo_native_contracts::Notary::script_hash(),
        )
        .expect("notary lookup before Echidna")
        .is_none(),
        "Notary must not deploy before its configured Echidna activation block"
    );

    let mut header = Header::new();
    header.set_index(1);
    let block = Arc::new(Block::from_parts(header, Vec::new()));
    let staged = stage_block_natives_with_resources(
        Arc::clone(&snapshot),
        block,
        &settings,
        NativePersistOptions {
            capture_replay_artifacts: false,
        },
        &resources,
    )
    .expect("Echidna block stages with reused resources");
    assert!(
        staged
            .outcome
            .initialized
            .iter()
            .any(|name| name == "Notary"),
        "reused resources must still recompute hardfork activation per block"
    );
    staged.commit();

    let notary = neo_native_contracts::ContractManagement::get_contract_from_snapshot(
        &snapshot,
        &neo_native_contracts::Notary::script_hash(),
    )
    .expect("notary lookup after Echidna")
    .expect("Notary deploys at Echidna");
    assert_eq!(notary.update_counter, 0);
}

/// Genesis persist now writes the C#-faithful Ledger records: the
/// `Prefix_Block` probe of [`chain_state_initialized`] (the literal
/// C# `Ledger.Initialized` check) and the current-block pointer.
#[test]
fn genesis_persist_writes_ledger_records() {
    let resources = standard_resources();
    let settings = ProtocolSettings::default();
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = Arc::new(genesis_block(&settings).expect("genesis block"));
    let genesis_hash = genesis.header.try_hash().unwrap();
    persist_with_resources(Arc::clone(&snapshot), genesis, &settings, &resources)
        .expect("genesis persist");

    let ledger = neo_native_contracts::LedgerContract::new();
    assert_eq!(
        ledger.get_block_hash(&snapshot, 0).unwrap(),
        Some(genesis_hash)
    );
    assert_eq!(ledger.current_index(&snapshot).unwrap(), 0);
    assert_eq!(ledger.current_hash(&snapshot).unwrap(), genesis_hash);
    let block_prefix = StorageKey::new(LEDGER_CONTRACT_ID, vec![LEDGER_PREFIX_BLOCK]);
    assert!(
        snapshot
            .find(
                Some(&block_prefix),
                neo_storage::persistence::SeekDirection::Forward
            )
            .next()
            .is_some(),
        "the C# Ledger.Initialized probe (any Prefix_Block record) must hit"
    );
}

/// The staging contract the per-block atomicity rests on: writes
/// into a `clone_cache()` child are invisible to the parent until
/// `commit()`, and dropping the child discards them. The persist
/// pipeline stages every block write in such a child, so a
/// mid-sequence error can never leave partial block state in the
/// caller's snapshot.
#[test]
fn block_staging_cache_isolates_until_commit() {
    let parent = DataCache::new(false);
    let key = StorageKey::new(-4, vec![5, 0xAA]);

    // Discard leg: child writes never reach the parent.
    {
        let child = parent.clone_cache();
        child.add(key.clone(), neo_storage::StorageItem::from_bytes(vec![1]));
        assert!(child.get(&key).is_some());
        assert!(parent.get(&key).is_none(), "uncommitted child write leaked");
    }
    assert!(parent.get(&key).is_none(), "dropped child write leaked");

    // Commit leg: the child write lands atomically on commit.
    let child = parent.clone_cache();
    child.add(key.clone(), neo_storage::StorageItem::from_bytes(vec![2]));
    assert!(parent.get(&key).is_none());
    child.commit();
    assert_eq!(
        parent.get(&key).map(|i| i.value_bytes().into_owned()),
        Some(vec![2])
    );
}
