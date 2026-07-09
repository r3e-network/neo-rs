use super::*;
use crate::NodeBuilder;
use neo_execution::native_contract_provider::NativeContractLookup;
use neo_storage::persistence::providers::memory_store::MemoryStore;

fn memory_store() -> Arc<dyn Store> {
    Arc::new(MemoryStore::new())
}

#[test]
fn builder_returns_node_builder() {
    let _: NodeBuilder = Node::builder();
}

#[test]
fn tx_admission_uses_ledger_provider_boundary() {
    let source = include_str!("../../composition/node.rs");
    let provider = include_str!("../../composition/tx_admission_provider.rs");
    let start = source
        .find("pub fn try_enqueue_preverify")
        .expect("try_enqueue_preverify exists");
    let body = &source[start..];

    assert!(
        body.contains("NativeTxAdmissionLedgerProviderFactory"),
        "composition-root tx admission must read ledger records through the tx-admission ledger provider factory"
    );
    assert!(
        !body.contains("StorageLedgerProviderFactory"),
        "composition-root tx admission must not construct storage ledger providers directly"
    );
    assert!(
        body.contains("NativeTxAdmissionProvider::new(self.mempool.native_contract_provider())"),
        "composition-root tx admission must adapt the mempool-captured native provider for policy reads"
    );
    assert!(
        !body.contains("NativeTxAdmissionProviderFactory"),
        "composition-root tx admission must not create a second production native provider factory"
    );
    assert!(
        !body.contains("LedgerContract::new()"),
        "composition-root tx admission must not construct native LedgerContract directly"
    );
    assert!(
        !body.contains("PolicyContract::new()"),
        "composition-root tx admission must not construct native PolicyContract directly"
    );
    assert!(provider.contains("trait TxAdmissionLedgerProvider"));
    assert!(provider.contains("trait TxAdmissionLedgerProviderFactory"));
    assert!(provider.contains("struct NativeTxAdmissionLedgerProviderFactory"));
    assert!(
        provider.contains("HotColdLedgerProviderFactory"),
        "the tx-admission ledger provider should use the routed ledger provider factory"
    );
    assert!(
        provider.contains("EmptyLedgerProvider"),
        "the tx-admission ledger provider should keep the no-cold-archive case explicit"
    );
    assert!(
        !provider.contains("StorageLedgerProviderFactory"),
        "the tx-admission ledger provider should not bypass the hot/cold provider boundary"
    );
    assert!(provider.contains("trait TxAdmissionNativeProvider"));
    assert!(
        !provider.contains("trait TxAdmissionNativeProviderFactory"),
        "tx admission native provider should adapt the node-composed NativeContractProvider instead of owning a private factory"
    );
    assert!(
        !provider.contains("struct NativeTxAdmissionProviderFactory"),
        "tx admission native provider should not own a second production native provider factory"
    );
    assert!(
        !provider.contains("PolicyContract::new()"),
        "tx admission native provider must resolve PolicyContract through the explicit native provider"
    );
    assert!(
        provider.contains("get_native_contract_by_name(\"PolicyContract\")"),
        "tx admission native provider should read PolicyContract from the explicit NativeContractProvider"
    );
}

#[tokio::test]
async fn cancellation_token_clone_is_independent() {
    let storage = memory_store();
    let settings = Arc::new(ProtocolSettings::default());
    let (bc, _rx) = BlockchainHandle::with_capacity();
    let (net, _nrx, _etx) = NetworkHandle::channel(8, 8);

    let node = NodeBuilder::default()
        .with_settings(settings)
        .with_storage(storage)
        .with_blockchain(bc)
        .with_network(net)
        .build()
        .expect("builder should succeed");

    let token = node.cancellation_token();
    token.cancel();
    assert!(node.shutdown.is_cancelled());
}

#[test]
fn direct_constructor_uses_builder_defaults() {
    let _guard = crate::composition::native_provider_test_guard();
    NativeContractLookup::replace_provider(None);

    let node = Node::new(Arc::new(ProtocolSettings::default()), None, None)
        .expect("headless node should use builder defaults");

    assert!(node.mempool.total_count() == 0);
    assert_eq!(node.header_cache.count(), 0);
    assert!(
        !node
            .native_contract_provider
            .all_native_contracts()
            .is_empty()
    );
    assert!(
        NativeContractLookup::native_contract_provider().is_none(),
        "Node::new should use a local provider through NodeBuilder defaults"
    );
}
