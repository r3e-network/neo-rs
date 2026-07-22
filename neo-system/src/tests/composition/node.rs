use super::*;
#[test]
fn tx_admission_uses_ledger_provider_boundary() {
    let source = include_str!("../../composition/node.rs");
    let start = source
        .find("pub fn submit_transaction")
        .expect("submit_transaction exists");
    let body = &source[start..];
    let signature = body
        .split_once('{')
        .map(|(signature, _)| signature)
        .expect("submit_transaction has a body");

    assert!(
        !signature.contains("snapshot"),
        "composition-root tx admission must not accept a caller-owned snapshot"
    );
    assert!(
        body.contains("StoreCache::<S>::new_from_snapshot(self.storage.snapshot())"),
        "composition-root tx admission must freeze canonical state at the admission boundary"
    );

    assert!(
        body.contains("self.ledger_provider_factory.provider(snapshot)"),
        "composition-root tx admission must use the node-wide routed Ledger provider factory"
    );
    assert!(
        body.contains("TransactionAdmissionLedger::new"),
        "composition must adapt the routed provider to neo-mempool's canonical admission capability"
    );
    assert!(
        body.contains(".add_transaction(origin, transaction, snapshot, &ledger)"),
        "composition must call the one typed mempool mutation boundary"
    );
    assert!(
        body.contains("origin.should_propagate()"),
        "network relay policy must be derived from the canonical transaction origin"
    );
    assert!(
        source.contains("HotColdLedgerProviderFactory<OptionalStaticLedgerProvider>"),
        "the node must retain one statically dispatched configurable Ledger factory"
    );
    for forbidden in [
        "try_enqueue_preverify",
        "TxRouterHandle",
        "NativeTxAdmissionProvider",
        "contains_transaction(&hash)",
        "contains_conflict_hash(&hash",
    ] {
        assert!(
            !source.contains(forbidden),
            "composition must not duplicate admission rule `{forbidden}`"
        );
    }
}

#[test]
fn test_constructor_uses_explicit_standard_components() {
    let node = Node::for_test(neo_config::NeoChainSpec::mainnet().expect("mainnet chain spec"));

    assert!(node.mempool.total_count() == 0);
    assert_eq!(node.header_cache.count(), 0);
    assert!(
        !node
            .native_contract_provider
            .all_native_contracts()
            .is_empty()
    );
}
