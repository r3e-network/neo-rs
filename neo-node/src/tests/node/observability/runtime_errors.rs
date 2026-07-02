use super::super::payloads::{
    ErrorReport, ObservabilityMetadata, build_better_stack_error_payload,
    build_generic_error_payload, build_google_error_payload, build_sentry_error_payload,
};

fn metadata() -> ObservabilityMetadata {
    ObservabilityMetadata {
        service_name: "neo-node-mainnet".to_string(),
        environment: Some("production".to_string()),
        node_id: Some("validator-1".to_string()),
        network: 0x334F_454E,
        version: "test",
    }
}

#[test]
fn runtime_error_report_is_provider_payload_compatible() {
    let error = anyhow::anyhow!("failed to bind 0.0.0.0:10333");
    let report = ErrorReport::runtime_error("p2p_listener", &error);

    assert_eq!(report.event_type, "runtime_error");
    assert_eq!(report.message, "p2p_listener: failed to bind 0.0.0.0:10333");
    assert!(report.location.is_none());

    let metadata = metadata();
    let generic = build_generic_error_payload(&metadata, &report);
    assert_eq!(generic["event"]["type"], "runtime_error");
    assert_eq!(
        generic["event"]["message"],
        "p2p_listener: failed to bind 0.0.0.0:10333"
    );

    let better_stack = build_better_stack_error_payload(&metadata, &report);
    assert_eq!(better_stack["level"], "error");
    assert_eq!(better_stack["event_type"], "runtime_error");
    assert_eq!(
        better_stack["message"],
        "p2p_listener: failed to bind 0.0.0.0:10333"
    );

    let sentry = build_sentry_error_payload(&metadata, &report);
    assert_eq!(sentry["level"], "error");
    assert_eq!(sentry["transaction"], "runtime_error");
    assert_eq!(sentry["tags"]["event_type"], "runtime_error");

    let google = build_google_error_payload(&metadata, &report);
    assert_eq!(
        google["message"],
        "runtime_error: p2p_listener: failed to bind 0.0.0.0:10333"
    );
    assert_eq!(
        google["context"]["reportLocation"]["functionName"],
        "runtime_error"
    );
}

#[test]
fn node_runtime_sources_report_seed_connection_failures_to_observability() {
    let seeds_source = include_str!("../../../node/seeds/mod.rs");
    let seed_dial_branch = seeds_source
        .split("seed dial failed")
        .nth(1)
        .expect("node source should contain seed dial failure branch");
    let seed_dns_branch = seeds_source
        .split("seed DNS resolution failed")
        .nth(1)
        .expect("node source should contain seed DNS failure branch");

    assert!(
        seed_dial_branch.contains("report_runtime_error")
            && seed_dial_branch.contains("\"seed_dial\""),
        "seed dial failures should notify configured observability error endpoints"
    );
    assert!(
        seed_dns_branch.contains("report_runtime_error")
            && seed_dns_branch.contains("\"seed_dns_resolution\""),
        "seed DNS failures should notify configured observability error endpoints"
    );
    assert!(
        seeds_source.contains("\"seed_no_addresses\""),
        "seed DNS results with no socket addresses should notify configured observability error endpoints"
    );
}

#[test]
fn node_runtime_sources_report_shutdown_signal_failures_to_observability() {
    let node_source = include_str!("../../../node/mod.rs");
    let shutdown_source = include_str!("../../../node/shutdown.rs");
    let shutdown_signal_branch = node_source
        .split("shutdown-signal handler failed")
        .nth(1)
        .expect("node source should contain shutdown-signal failure branch");

    assert!(
        shutdown_signal_branch.contains("report_runtime_error")
            && shutdown_signal_branch.contains("\"shutdown_signal\""),
        "shutdown-signal handler failures should notify configured observability error endpoints"
    );
    assert!(
        shutdown_source.contains("essential_task"),
        "shutdown helper should treat essential task failure as a first-class graceful shutdown signal"
    );
}
