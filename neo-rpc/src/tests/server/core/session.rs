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
    let session_dummy_block = include_str!("../../../server/session/dummy_block.rs");
    assert!(
        session_dummy_block.contains("NativeSessionLedgerProviderFactory"),
        "session dummy block should read ledger records through the session ledger provider factory"
    );
    assert!(
        !session_dummy_block.contains("StorageLedgerProviderFactory"),
        "session dummy block should not construct storage ledger providers directly"
    );
    assert!(
        !session_dummy_block.contains("LedgerContract::new()"),
        "session dummy block should not construct native LedgerContract directly"
    );

    let current_height_sources = [
        (
            "wallet balance",
            include_str!("../../../server/rpc_server_wallet/balance.rs"),
        ),
        (
            "smart-contract unclaimed gas",
            include_str!("../../../server/smart_contract/unclaimed_gas.rs"),
        ),
        (
            "wallet compat network fee",
            include_str!("../../../server/wallet_compat/network_fee.rs"),
        ),
        (
            "wallet compat transaction builder",
            include_str!("../../../server/wallet_compat/transaction_builder.rs"),
        ),
        (
            "session execution",
            include_str!("../../../server/session/execution.rs"),
        ),
        (
            "smart-contract wallet invocation",
            include_str!("../../../server/smart_contract/invocation_wallet.rs"),
        ),
    ];

    for (name, source) in current_height_sources {
        assert!(
            source.contains("ledger_queries::current_index"),
            "{name} should route current-height reads through the shared ledger-query boundary"
        );
        assert!(
            !source.contains("StorageLedgerProviderFactory"),
            "{name} should not construct the storage ledger provider directly for current-height reads"
        );
        assert!(
            !source.contains("LedgerContract::new()"),
            "{name} should not construct native LedgerContract directly"
        );
    }

    let blockchain_mempool = include_str!("../../../server/rpc_server_blockchain/mempool.rs");
    assert!(
        blockchain_mempool.contains("NativeBlockchainLedgerProviderFactory"),
        "getrawmempool verbose mode should read height through the blockchain ledger provider factory"
    );
    assert!(
        !blockchain_mempool.contains("StorageLedgerProviderFactory"),
        "getrawmempool should not construct the storage ledger provider directly"
    );

    let blockchain_native = include_str!("../../../server/rpc_server_blockchain/native.rs");
    assert!(
        blockchain_native.contains("NativeBlockchainLedgerProviderFactory"),
        "getnativecontracts should read height through the blockchain ledger provider factory"
    );
    assert!(
        !blockchain_native.contains("StorageLedgerProviderFactory"),
        "getnativecontracts should not construct the storage ledger provider directly"
    );

    let blockchain_transactions =
        include_str!("../../../server/rpc_server_blockchain/transactions.rs");
    assert!(
        blockchain_transactions.contains("NativeBlockchainLedgerProviderFactory"),
        "transaction RPC handlers should read transaction state through the blockchain ledger provider factory"
    );
    assert!(
        !blockchain_transactions.contains("StorageLedgerProviderFactory"),
        "transaction RPC handlers should not construct the storage ledger provider directly"
    );

    let blockchain_responses = include_str!("../../../server/rpc_server_blockchain/responses.rs");
    assert!(
        blockchain_responses.contains("ledger_queries::transaction_context"),
        "verbose transaction response context should use the shared ledger-query boundary"
    );
    assert!(
        !blockchain_responses.contains("StorageLedgerProviderFactory"),
        "blockchain response projection should not construct storage ledger providers directly"
    );

    let blockchain_provider =
        include_str!("../../../server/rpc_server_blockchain/ledger_provider.rs");
    assert!(blockchain_provider.contains("trait BlockchainLedgerProvider"));
    assert!(blockchain_provider.contains("trait BlockchainLedgerProviderFactory"));
    assert!(blockchain_provider.contains("fn transaction_state_by_hash"));
    assert!(blockchain_provider.contains("struct NativeBlockchainLedgerProviderFactory"));
    assert!(
        blockchain_provider.contains("ledger_queries::current_index"),
        "blockchain current-height reads should use the shared ledger-query boundary"
    );
    assert!(
        blockchain_provider.contains("StorageLedgerProviderFactory"),
        "blockchain transaction-state reads still belong to the local provider adapter"
    );

    let wallet_transfers = include_str!("../../../server/rpc_server_wallet/transfers.rs");
    assert!(
        wallet_transfers.contains("NativeWalletLedgerProviderFactory"),
        "wallet transfers should read transaction state through the wallet ledger provider factory"
    );
    assert!(
        !wallet_transfers.contains("StorageLedgerProviderFactory"),
        "wallet transfers should not construct storage ledger providers directly"
    );

    let wallet_provider = include_str!("../../../server/rpc_server_wallet/ledger_provider.rs");
    assert!(wallet_provider.contains("trait WalletLedgerProvider"));
    assert!(wallet_provider.contains("trait WalletLedgerProviderFactory"));
    assert!(wallet_provider.contains("fn transaction_state_by_hash"));
    assert!(wallet_provider.contains("struct NativeWalletLedgerProviderFactory"));
    assert!(
        wallet_provider.contains("StorageLedgerProviderFactory"),
        "wallet transaction-state reads still belong to the local provider adapter"
    );

    let session_provider = include_str!("../../../server/session/ledger_provider.rs");
    assert!(session_provider.contains("trait SessionLedgerProvider"));
    assert!(session_provider.contains("trait SessionLedgerProviderFactory"));
    assert!(session_provider.contains("fn current_header"));
    assert!(session_provider.contains("struct NativeSessionLedgerProviderFactory"));
    assert!(
        session_provider.contains("StorageLedgerProviderFactory"),
        "session current-header reads still belong to the local provider adapter"
    );
}

#[test]
fn rpc_session_policy_reads_use_composed_native_provider() {
    let provider = include_str!("../../../server/session/native_provider.rs");
    assert!(provider.contains("trait SessionNativeProvider"));
    assert!(provider.contains("struct NativeSessionProvider"));
    assert!(
        provider.contains("native_contract_provider: Arc<dyn NativeContractProvider>"),
        "session native provider should adapt the composition-root provider"
    );
    assert!(
        provider.contains("get_native_contract_by_name(\"PolicyContract\")"),
        "session native provider should resolve Policy through NativeContractProvider"
    );
    assert!(
        provider.contains("with_contract::<PolicyContract"),
        "session native provider should downcast through the shared native provider adapter"
    );
    assert!(
        !provider.contains("PolicyContract::new()"),
        "session native provider should not construct a standalone PolicyContract"
    );
    assert!(
        !provider.contains("SessionNativeProviderFactory"),
        "session native provider should be created from the composed provider, not a local factory"
    );

    let session_execution = include_str!("../../../server/session/execution.rs");
    assert!(
        session_execution
            .contains("NativeSessionProvider::new(Arc::clone(&native_contract_provider))"),
        "session execution should adapt the composed native provider once"
    );
    assert!(
        !session_execution.contains("PolicyContract::new()"),
        "session execution should not construct PolicyContract directly"
    );
    assert!(
        !session_execution.contains("NativeSessionProviderFactory"),
        "session execution should not create a standalone session native provider factory"
    );

    let dummy_block = include_str!("../../../server/session/dummy_block.rs");
    assert!(
        dummy_block.contains("native_provider: &impl SessionNativeProvider"),
        "dummy block construction should receive the narrow native provider capability"
    );
    assert!(
        dummy_block.contains("let milliseconds_per_block = native_provider")
            && dummy_block.contains(".milliseconds_per_block(snapshot, settings)"),
        "dummy block construction should read Policy data through the supplied provider"
    );
    assert!(
        !dummy_block.contains("PolicyContract::new()"),
        "session dummy block should not construct PolicyContract directly"
    );
    assert!(
        !dummy_block.contains("NativeSessionProviderFactory"),
        "session dummy block should not create a standalone session native provider factory"
    );
}

#[test]
fn smart_contract_wallet_policy_reads_use_composed_native_provider() {
    let provider = include_str!("../../../server/smart_contract/native_provider.rs");
    assert!(provider.contains("trait SmartContractNativeProvider"));
    assert!(provider.contains("struct NativeSmartContractProvider"));
    assert!(
        provider.contains("native_contract_provider: Arc<dyn NativeContractProvider>"),
        "smart-contract native provider should adapt the composition-root provider"
    );
    assert!(
        provider.contains("get_native_contract_by_name(\"PolicyContract\")"),
        "smart-contract native provider should resolve Policy through NativeContractProvider"
    );
    assert!(
        provider.contains("with_contract::<PolicyContract"),
        "smart-contract native provider should downcast through the shared native provider adapter"
    );
    assert!(
        !provider.contains("PolicyContract::new()"),
        "smart-contract native provider should not construct a standalone PolicyContract"
    );
    assert!(
        !provider.contains("SmartContractNativeProviderFactory"),
        "smart-contract native provider should be created from the composed provider, not a local factory"
    );

    let invocation_wallet = include_str!("../../../server/smart_contract/invocation_wallet.rs");
    assert!(
        invocation_wallet
            .contains("NativeSmartContractProvider::new(Arc::clone(&native_contract_provider))"),
        "wallet invoke tx materialization should adapt the composed native provider"
    );
    assert!(
        !invocation_wallet.contains("PolicyContract::new()"),
        "wallet invoke tx materialization should not construct PolicyContract directly"
    );
    assert!(
        !invocation_wallet.contains("NativeSmartContractProviderFactory"),
        "wallet invoke tx materialization should not create a standalone smart-contract native provider factory"
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

#[test]
fn rpc_wallet_policy_reads_use_native_provider_factory() {
    let provider = include_str!("../../../server/rpc_server_wallet/native_provider.rs");
    assert!(provider.contains("trait WalletNativeProvider"));
    assert!(provider.contains("trait WalletNativeProviderFactory"));
    assert!(provider.contains("struct NativeWalletProviderFactory"));
    assert!(provider.contains("PolicyContract::new()"));

    let signing = include_str!("../../../server/rpc_server_wallet/signing.rs");
    assert!(
        signing.contains("NativeWalletProviderFactory"),
        "wallet signing should read Policy values through the wallet native provider factory"
    );
    assert!(
        !signing.contains("PolicyContract::new()"),
        "wallet signing should not construct PolicyContract directly"
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
