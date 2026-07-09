#[path = "observability/endpoints.rs"]
mod endpoints;
#[path = "observability/health.rs"]
mod health;
#[path = "observability/payloads.rs"]
mod payloads;
#[path = "observability/redaction.rs"]
mod redaction;
#[path = "observability/retry.rs"]
mod retry;
#[path = "observability/runtime_config.rs"]
mod runtime_config;
#[path = "observability/runtime_config_validation.rs"]
mod runtime_config_validation;
#[path = "observability/runtime_errors.rs"]
mod runtime_errors;
#[path = "observability/runtime_lifecycle.rs"]
mod runtime_lifecycle;
#[path = "observability/task_monitoring.rs"]
mod task_monitoring;

#[test]
fn observability_runtime_source_does_not_panic_on_recoverable_state() {
    let source = include_str!("../../node/observability.rs");
    for forbidden in [".expect(", ".unwrap(", "panic!", "todo!", "unimplemented!"] {
        assert!(
            !source.contains(forbidden),
            "observability runtime should return errors instead of using {forbidden}"
        );
    }
}

#[test]
fn observability_ledger_height_uses_routed_provider_shape() {
    let source = include_str!("../../node/observability/ledger_provider.rs");

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
