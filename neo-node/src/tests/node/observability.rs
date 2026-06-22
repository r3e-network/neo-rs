mod endpoints;
mod health;
mod payloads;
mod redaction;
mod retry;
mod runtime_config;
mod runtime_config_validation;
mod runtime_errors;
mod runtime_lifecycle;
mod task_monitoring;

#[test]
fn observability_runtime_source_does_not_panic_on_recoverable_state() {
    let source = include_str!("../observability.rs");
    for forbidden in [".expect(", ".unwrap(", "panic!", "todo!", "unimplemented!"] {
        assert!(
            !source.contains(forbidden),
            "observability runtime should return errors instead of using {forbidden}"
        );
    }
}
