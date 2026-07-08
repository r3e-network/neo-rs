use super::*;
use neo_config::ProtocolSettings;
use neo_execution::iterators::{IteratorInterop, StorageIterator};
use neo_primitives::FindOptions;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::stack_item::{InteropInterface as VmInteropInterface, StackItem};
use neo_vm_rs::{OpCode, VmState};
use std::sync::Arc;

#[test]
fn server_context_engine_paths_use_explicit_native_provider() {
    let sources = [
        (
            "rpc wallet balance",
            include_str!("../../../server/rpc_server_wallet/balance.rs"),
        ),
        (
            "native queries",
            include_str!("../../../server/native_queries/execution.rs"),
        ),
        (
            "contract verify",
            include_str!("../../../server/smart_contract/contract_verify.rs"),
        ),
        (
            "token tracker properties",
            include_str!("../../../server/rpc_server_tokens_tracker/properties.rs"),
        ),
        (
            "token tracker helpers",
            include_str!("../../../server/rpc_server_tokens_tracker/helpers.rs"),
        ),
        (
            "wallet compat probes",
            include_str!("../../../server/wallet_compat/probes.rs"),
        ),
        (
            "wallet compat network fee",
            include_str!("../../../server/wallet_compat/network_fee.rs"),
        ),
        (
            "session",
            include_str!("../../../server/session/execution.rs"),
        ),
    ];

    for (name, source) in sources {
        assert!(
            source.contains("new_with_shared_block_and_native_contract_provider"),
            "{name} should construct ApplicationEngine with an explicit native provider"
        );
        assert!(
            source.contains("native_contract_provider"),
            "{name} should pass the composed native provider"
        );
        assert!(
            !source.contains("ApplicationEngine::new("),
            "{name} should not read the ambient native-provider bridge"
        );
    }

    let provider_threading_sources = [(
        "wallet compat transaction builder",
        include_str!("../../../server/wallet_compat/transaction_builder.rs"),
    )];
    for (name, source) in provider_threading_sources {
        assert!(
            source.contains("native_contract_provider"),
            "{name} should thread the composed native provider through wallet probes"
        );
        assert!(
            !source.contains("ApplicationEngine::new("),
            "{name} should not construct engines through the ambient provider bridge"
        );
    }
}

#[test]
fn rpc_server_ledger_reads_use_provider_boundaries() {
    let sources = [
        (
            "blockchain mempool",
            include_str!("../../../server/rpc_server_blockchain/mempool.rs"),
        ),
        (
            "blockchain native contracts",
            include_str!("../../../server/rpc_server_blockchain/native.rs"),
        ),
        (
            "session dummy block",
            include_str!("../../../server/session/dummy_block.rs"),
        ),
        (
            "session execution",
            include_str!("../../../server/session/execution.rs"),
        ),
        (
            "wallet balance",
            include_str!("../../../server/rpc_server_wallet/balance.rs"),
        ),
        (
            "wallet transfers",
            include_str!("../../../server/rpc_server_wallet/transfers.rs"),
        ),
        (
            "smart-contract unclaimed gas",
            include_str!("../../../server/smart_contract/unclaimed_gas.rs"),
        ),
        (
            "smart-contract wallet invocation",
            include_str!("../../../server/smart_contract/invocation_wallet.rs"),
        ),
        (
            "wallet compat network fee",
            include_str!("../../../server/wallet_compat/network_fee.rs"),
        ),
        (
            "wallet compat transaction builder",
            include_str!("../../../server/wallet_compat/transaction_builder.rs"),
        ),
    ];

    for (name, source) in sources {
        assert!(
            source.contains("StorageLedgerProviderFactory"),
            "{name} should read ledger records through the provider factory"
        );
        assert!(
            !source.contains("LedgerContract::new()"),
            "{name} should not construct native LedgerContract directly"
        );
    }
}

#[test]
fn rpc_session_policy_reads_use_native_provider_factory() {
    let provider = include_str!("../../../server/session/native_provider.rs");
    assert!(provider.contains("trait SessionNativeProvider"));
    assert!(provider.contains("trait SessionNativeProviderFactory"));
    assert!(provider.contains("struct NativeSessionProviderFactory"));
    assert!(provider.contains("PolicyContract::new()"));

    let session_sources = [
        (
            "session dummy block",
            include_str!("../../../server/session/dummy_block.rs"),
        ),
        (
            "session execution",
            include_str!("../../../server/session/execution.rs"),
        ),
    ];

    for (name, source) in session_sources {
        assert!(
            source.contains("NativeSessionProviderFactory"),
            "{name} should obtain Policy values through the session native provider factory"
        );
        assert!(
            !source.contains("PolicyContract::new()"),
            "{name} should not construct PolicyContract directly"
        );
    }
}

#[test]
fn smart_contract_wallet_policy_reads_use_native_provider_factory() {
    let provider = include_str!("../../../server/smart_contract/native_provider.rs");
    assert!(provider.contains("trait SmartContractNativeProvider"));
    assert!(provider.contains("trait SmartContractNativeProviderFactory"));
    assert!(provider.contains("struct NativeSmartContractProviderFactory"));
    assert!(provider.contains("PolicyContract::new()"));

    let invocation_wallet = include_str!("../../../server/smart_contract/invocation_wallet.rs");
    assert!(
        invocation_wallet.contains("NativeSmartContractProviderFactory"),
        "wallet invoke tx materialization should read Policy values through the smart-contract native provider factory"
    );
    assert!(
        !invocation_wallet.contains("PolicyContract::new()"),
        "wallet invoke tx materialization should not construct PolicyContract directly"
    );
}

#[test]
fn wallet_compat_policy_reads_use_native_provider_factory() {
    let provider = include_str!("../../../server/wallet_compat/native_provider.rs");
    assert!(provider.contains("trait WalletCompatNativeProvider"));
    assert!(provider.contains("trait WalletCompatNativeProviderFactory"));
    assert!(provider.contains("struct NativeWalletCompatProviderFactory"));
    assert!(provider.contains("PolicyContract::new()"));

    let network_fee = include_str!("../../../server/wallet_compat/network_fee.rs");
    assert!(
        network_fee.contains("NativeWalletCompatProviderFactory"),
        "wallet network-fee calculation should read Policy values through the wallet-compat native provider factory"
    );
    assert!(
        !network_fee.contains("PolicyContract::new()"),
        "wallet network-fee calculation should not construct PolicyContract directly"
    );
}

/// Genesis-block timestamp seeded by the RPC test harness
/// (`seed_genesis_state` / `genesis_header`).
const GENESIS_TIMESTAMP: u64 = 1_468_595_301_000;

/// FIX 13 — a stateless RPC invoke must run against a *dummy persisting block*
/// (C# `ApplicationEngine.CreateDummyBlock`), so `System.Runtime.GetTime` does
/// not fault and reports `prevBlock.Timestamp + msPerBlock`, and index-dependent
/// reads see `height + 1`.
#[tokio::test(flavor = "multi_thread")]
async fn stateless_invoke_builds_dummy_persisting_block() {
    let settings = ProtocolSettings::default();
    let expected_timestamp = GENESIS_TIMESTAMP + u64::from(settings.milliseconds_per_block);
    let system = crate::server::test_support::test_system(settings);

    // SYSCALL System.Runtime.GetTime ; RET
    let mut script = vec![OpCode::SYSCALL.byte()];
    script.extend_from_slice(&neo_vm_rs::interop_hash("System.Runtime.GetTime").to_le_bytes());
    script.push(OpCode::RET.byte());

    let session = Session::new(
        system.clone(),
        system.clone(),
        system.native_contract_provider(),
        script,
        None,
        None,
        100_000_000,
        None,
    )
    .expect("session");

    let engine = session.engine();

    // Before the fix the engine had no persisting block: GetTime faulted.
    assert_eq!(
        engine.state(),
        VmState::HALT,
        "GetTime must not fault with a dummy persisting block"
    );

    // Dummy block index = currentBlock.Index + 1 (genesis 0 -> 1).
    let block = engine
        .persisting_block()
        .expect("dummy persisting block present");
    assert_eq!(block.index(), 1, "dummy block index = height + 1");
    assert_eq!(block.timestamp(), expected_timestamp);
    assert_eq!(block.version(), 0);
    assert_eq!(block.merkle_root(), &neo_primitives::UInt256::default());
    assert_eq!(engine.current_block_index(), 1);

    // GetTime result on the stack == prevBlock.Timestamp + msPerBlock.
    let top = engine
        .result_stack()
        .peek(0)
        .expect("GetTime result on stack");
    let time = top.as_int().expect("integer result");
    assert_eq!(time, num_bigint::BigInt::from(expected_timestamp));
}

/// FIX 17a — the session's synthetic transaction container derives
/// `ValidUntilBlock` from the *Policy-aware* MaxValidUntilBlockIncrement
/// (`snapshot.GetMaxValidUntilBlockIncrement(settings)`). With default settings
/// (HF_Echidna disabled) this equals the static protocol setting, applied over
/// the current height.
#[tokio::test(flavor = "multi_thread")]
async fn session_valid_until_block_uses_policy_aware_increment() {
    let settings = ProtocolSettings::default();
    let increment = settings.max_valid_until_block_increment;
    let system = crate::server::test_support::test_system(settings);

    let signer = neo_payloads::Signer::new(
        neo_primitives::UInt160::default(),
        neo_payloads::WitnessScope::CALLED_BY_ENTRY,
    );

    let session = Session::new(
        system.clone(),
        system.clone(),
        system.native_contract_provider(),
        vec![OpCode::RET.byte()],
        Some(vec![signer]),
        None,
        100_000_000,
        None,
    )
    .expect("session");

    let engine = session.engine();
    let container = engine.script_container().expect("tx container present");
    let tx = container
        .as_any()
        .downcast_ref::<neo_payloads::transaction::Transaction>()
        .expect("container is a transaction");

    // current height (genesis) = 0, so ValidUntilBlock = 0 + increment.
    assert_eq!(tx.valid_until_block(), increment);
}

#[tokio::test(flavor = "multi_thread")]
async fn session_registers_and_traverses_storage_iterator() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings);
    let session = Session::new(
        system.clone(), // Arc<NodeContext> coerced to Arc<dyn StoreProvider>
        system.clone(), // Arc<NodeContext> coerced to Arc<dyn ConfigProvider>
        system.native_contract_provider(),
        vec![OpCode::RET.byte()],
        None,
        None,
        100_000_000,
        None,
    )
    .expect("session");

    let entries = vec![
        (
            StorageKey::new(1, vec![0x01]),
            StorageItem::from_bytes(vec![0xAA]),
        ),
        (
            StorageKey::new(1, vec![0x02]),
            StorageItem::from_bytes(vec![0xBB]),
        ),
    ];
    let iterator = StorageIterator::new(entries, 0, FindOptions::None);
    let iterator_id = {
        let mut engine = session.engine_mut();
        engine
            .store_storage_iterator(iterator)
            .expect("store iterator")
    };

    let interop = Arc::new(IteratorInterop::new(iterator_id)) as Arc<dyn VmInteropInterface>;
    let uuid_first = session
        .register_iterator_interface(&interop)
        .expect("iterator registered");
    let uuid_second = session
        .register_iterator_interface(&interop)
        .expect("iterator re-registered");
    assert_eq!(uuid_first, uuid_second);
    assert!(session.has_iterators());

    let values = session
        .traverse_iterator(&uuid_first, 10)
        .expect("traverse iterator");
    assert_eq!(values.len(), 2);
    assert!(matches!(values[0], StackItem::Struct(_)));

    let tail = session
        .traverse_iterator(&uuid_first, 10)
        .expect("traverse iterator exhausted");
    assert!(tail.is_empty());
}
