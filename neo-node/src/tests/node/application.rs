//! Application workflow boundary tests.

use std::path::PathBuf;

use crate::node::application::NodeCommand;
use crate::node::cli::NodeCli;

#[test]
fn node_command_rejects_conflicting_modes_before_runtime_open() {
    let command = NodeCommand::from_cli(NodeCli {
        config: PathBuf::from("neo_testnet_node.toml"),
        network_magic: None,
        storage_path: None,
        check_config: false,
        check_storage: false,
        check_all: false,
        import_chain: Some(PathBuf::from("chain.acc")),
        fast_sync: false,
        fast_sync_cache: None,
        fast_sync_reference_rpc: None,
        fast_sync_report: None,
        fast_sync_expected_sha256: None,
        stop_at_height: None,
        remote_ledger_rpc: Some("http://127.0.0.1:10332".to_string()),
    });

    let error = command.expect_err("conflicting node modes must fail before opening runtime");
    assert!(
        error.to_string().contains("--remote-ledger-rpc"),
        "unexpected validation error: {error}"
    );
}

#[test]
fn daemon_entrypoint_stays_at_application_abstraction_level() {
    let source = include_str!("../../node/lifecycle/daemon.rs");

    for required in [
        "NodeCommand::from_cli",
        ".open_runtime()",
        ".run_requested_mode()",
    ] {
        assert!(
            source.contains(required),
            "daemon entrypoint should expose application step `{required}`"
        );
    }

    for forbidden in [
        "build_node(",
        "run_startup_imports(",
        "start_live_services(",
        "run_daemon_shutdown(",
        "StoreCache",
        "CancellationToken",
    ] {
        assert!(
            !source.contains(forbidden),
            "daemon entrypoint must hide lower-layer mechanic `{forbidden}`"
        );
    }
}

#[test]
fn node_composition_delegates_provider_neutral_core_assembly() {
    let source = include_str!("../../node/lifecycle/composition.rs");

    assert!(
        source.contains("NodeCoreBuilder::new"),
        "the application composition should enter core assembly through neo-system"
    );

    for forbidden in [
        "StoreCache::new_from_store",
        "MemoryPool::new_with_native_contract_provider",
        "HeaderCache::default",
        "LedgerContext::default",
        "NodeSystemContext::new",
        "BlockchainService::with_defaults",
        "neo_system::Node::builder",
    ] {
        assert!(
            !source.contains(forbidden),
            "provider-neutral core constructor `{forbidden}` belongs in neo-system"
        );
    }
}

#[test]
fn opened_runtime_delegates_to_the_running_node_workflow() {
    let source = include_str!("../../node/application/runtime.rs");
    let compact = source
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect::<String>();

    assert!(
        compact.contains("running_node.run_requested_mode("),
        "the opened application runtime should execute one named node workflow"
    );
    for forbidden in [
        "let RunningNode {",
        "StartupImportContext {",
        "run_startup_imports(",
        "start_live_services(",
        "run_daemon_shutdown(",
    ] {
        assert!(
            !source.contains(forbidden),
            "application lifecycle must hide lower workflow mechanic `{forbidden}`"
        );
    }
}

#[test]
fn startup_metrics_precede_bulk_import_without_starting_live_network_services() {
    let workflow = include_str!("../../node/lifecycle/workflow.rs");
    let metrics = workflow
        .find("start_metrics_endpoint(")
        .expect("running-node workflow starts local metrics");
    let imports = workflow
        .find("run_startup_imports(")
        .expect("running-node workflow runs startup imports");
    let live_services = workflow
        .find("start_live_services(")
        .expect("running-node workflow starts post-import live services");

    assert!(metrics < imports, "metrics must observe startup imports");
    assert!(
        imports < live_services,
        "P2P and RPC services must remain post-import"
    );

    let live_services_source = include_str!("../../node/lifecycle/live_services.rs");
    let live_services_body = live_services_source
        .split("pub(in crate::node) async fn start_live_services")
        .nth(1)
        .expect("live-service function body");
    let live_services_body = live_services_body
        .split("pub(in crate::node) fn start_metrics_endpoint")
        .next()
        .expect("metrics helper follows live-service function");
    assert!(
        !live_services_body.contains("start_metrics_endpoint("),
        "post-import live services must not bind the metrics endpoint twice"
    );
}
