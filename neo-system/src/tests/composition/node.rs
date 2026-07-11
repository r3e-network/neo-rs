use super::*;
use crate::NodeBuilder;

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
        body.contains("self.ledger_provider_factory.provider(snapshot)"),
        "composition-root tx admission must use the node-wide routed Ledger provider factory"
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
    assert!(
        source.contains("HotColdLedgerProviderFactory<OptionalStaticLedgerProvider>"),
        "the node must retain one statically dispatched configurable Ledger factory"
    );
    assert!(
        !provider.contains("TxAdmissionLedgerProvider"),
        "the obsolete private Ledger wrapper should stay deleted"
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
        provider.contains(".max_traceable_blocks(snapshot, settings)"),
        "tx admission native provider should read MaxTraceableBlocks from the explicit NativeContractProvider capability"
    );
}

#[test]
fn direct_constructor_uses_builder_defaults() {
    let node = Node::new(Arc::new(ProtocolSettings::default()), None, None)
        .expect("headless node should use explicit standard provider defaults");

    assert!(node.mempool.total_count() == 0);
    assert_eq!(node.header_cache.count(), 0);
    assert!(
        !node
            .native_contract_provider
            .all_native_contracts()
            .is_empty()
    );
}
