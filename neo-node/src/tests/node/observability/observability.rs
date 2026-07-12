#[path = "transport/endpoints.rs"]
mod endpoints;
#[path = "health.rs"]
mod health;
#[path = "projection/payloads.rs"]
mod payloads;
#[path = "transport/redaction.rs"]
mod redaction;
#[path = "transport/retry.rs"]
mod retry;
#[path = "runtime/runtime_config.rs"]
mod runtime_config;
#[path = "runtime/runtime_config_validation.rs"]
mod runtime_config_validation;
#[path = "runtime/runtime_errors.rs"]
mod runtime_errors;
#[path = "runtime/runtime_lifecycle.rs"]
mod runtime_lifecycle;
#[path = "runtime/task_monitoring.rs"]
mod task_monitoring;

#[test]
fn observability_runtime_source_does_not_panic_on_recoverable_state() {
    let source = include_str!("../../../node/observability.rs");
    for forbidden in [".expect(", ".unwrap(", "panic!", "todo!", "unimplemented!"] {
        assert!(
            !source.contains(forbidden),
            "observability runtime should return errors instead of using {forbidden}"
        );
    }
}

#[test]
fn observability_ledger_height_uses_routed_provider_shape() {
    let source = include_str!("../../../node/observability/ledger_provider.rs");

    assert!(
        source.contains("HotColdLedgerProviderFactory"),
        "observability ledger height should use the routed ledger provider factory"
    );
    assert!(
        source.contains("EmptyLedgerProvider"),
        "observability ledger height should keep the no-cold-archive case explicit"
    );
    assert!(
        !source.contains("StorageLedgerProviderFactory"),
        "observability ledger height should not bypass the hot/cold ledger provider boundary"
    );
}
