use super::*;
use crate::cli::NodeCli;
use crate::tee_integration::{PrivacyEncryption, TeeRuntime};
use clap::Parser;
use neo_protocol::ProtocolSettings;

fn privacy_config(mode: &str, key_hex: Option<&str>) -> NodeConfig {
    let extra = key_hex
        .map(|key| format!("committee_epoch_key_hex = \"{key}\"\n"))
        .unwrap_or_default();
    let source = format!("[privacy]\nenabled = true\nencryption_mode = \"{mode}\"\n{extra}");
    toml::from_str(&source).expect("parse privacy config")
}

/// `expect_err` without the `Debug` bound on the runtime handle.
fn expect_startup_err(
    result: anyhow::Result<Option<Arc<crate::tee_integration::TeeRuntime>>>,
    message: &str,
) -> anyhow::Error {
    match result {
        Ok(_) => panic!("{message}"),
        Err(err) => err,
    }
}

fn node_server(system: &Arc<NeoSystem>) -> RpcServer {
    let mut server = RpcServer::new(
        Arc::clone(system),
        neo_rpc::server::RpcServerConfig::default(),
    );
    server.register_handlers(RpcServerNode::register_handlers());
    server
}

#[tokio::test(flavor = "multi_thread")]
async fn privacy_rpc_endpoint_absent_without_runtime() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut server = node_server(&system);
    assert!(
        server.is_method_registered("sendrawtransaction"),
        "standard node handlers must stay registered"
    );

    let installed = install_privacy_rpc_endpoint(&mut server, None).expect("install");
    assert!(
        !installed,
        "no TEE runtime must not register the privacy endpoint"
    );
    assert!(!server.is_method_registered("sendrawtransactionencrypted"));
    assert!(!server.is_privacy_submission_enabled());

    system.shutdown().await.expect("shutdown");
}

#[cfg(not(feature = "tee-sgx"))]
#[tokio::test(flavor = "multi_thread")]
async fn privacy_rpc_endpoint_absent_when_privacy_disabled() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut server = node_server(&system);

    let temp = tempfile::tempdir().expect("temp dir");
    let runtime =
        Arc::new(TeeRuntime::new(temp.path().to_path_buf(), "batched", 1000).expect("runtime"));

    let installed = install_privacy_rpc_endpoint(&mut server, Some(&runtime)).expect("install");
    assert!(
        !installed,
        "a privacy-disabled runtime must not register the privacy endpoint"
    );
    assert!(!server.is_method_registered("sendrawtransactionencrypted"));
    assert!(!server.is_privacy_submission_enabled());

    runtime.shutdown().expect("runtime shutdown");
    system.shutdown().await.expect("shutdown");
}

#[cfg(not(feature = "tee-sgx"))]
#[tokio::test(flavor = "multi_thread")]
async fn privacy_rpc_endpoint_registered_when_privacy_enabled() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let mut server = node_server(&system);

    let temp = tempfile::tempdir().expect("temp dir");
    let runtime = Arc::new(
        TeeRuntime::with_privacy(
            temp.path().to_path_buf(),
            "batched",
            1000,
            PrivacyEncryption::TeeSealedKey,
        )
        .expect("privacy runtime"),
    );

    let installed = install_privacy_rpc_endpoint(&mut server, Some(&runtime)).expect("install");
    assert!(
        installed,
        "a privacy-enabled runtime must register the encrypted endpoint"
    );
    assert!(server.is_method_registered("sendrawtransactionencrypted"));
    assert!(server.is_privacy_submission_enabled());

    runtime.shutdown().expect("runtime shutdown");
    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn privacy_enabled_without_tee_flags_fails_closed() {
    let cli = NodeCli::parse_from(["neo-node"]);
    let node_config = privacy_config("tee_sealed_key", None);
    let protocol_settings = node_config.protocol_settings();
    let system = NeoSystem::new(protocol_settings.clone(), None, None).expect("neo system");

    let err = expect_startup_err(
        maybe_enable_tee_runtime(&cli, &node_config, &protocol_settings, &system),
        "privacy without TEE flags must fail closed",
    );
    assert!(
        err.to_string().contains("privacy mode is enabled"),
        "unexpected error: {err}"
    );
    assert!(
        err.to_string().contains("--tee"),
        "the error must tell the operator how to opt in: {err}"
    );

    system.shutdown().await.expect("shutdown");
}

#[cfg(not(feature = "tee-sgx"))]
#[tokio::test(flavor = "multi_thread")]
async fn privacy_with_tee_flag_initializes_encrypted_runtime() {
    let temp = tempfile::tempdir().expect("temp dir");
    let tee_path = temp.path().to_string_lossy().to_string();
    let cli = NodeCli::parse_from(["neo-node", "--tee", "--tee-data-path", tee_path.as_str()]);
    let node_config = privacy_config("tee_sealed_key", None);
    let protocol_settings = node_config.protocol_settings();
    let system = NeoSystem::new(protocol_settings.clone(), None, None).expect("neo system");

    let runtime = maybe_enable_tee_runtime(&cli, &node_config, &protocol_settings, &system)
        .expect("privacy + --tee must initialize the encrypted runtime")
        .expect("runtime");
    assert!(runtime.privacy_enabled());
    assert!(runtime.intake_kem().is_some());

    // Never leave a registered global runtime behind for other tests.
    crate::tee_integration::clear_active_runtime();
    runtime.shutdown().expect("runtime shutdown");
    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn privacy_committee_key_missing_epoch_key_fails_startup() {
    let temp = tempfile::tempdir().expect("temp dir");
    let tee_path = temp.path().to_string_lossy().to_string();
    let cli = NodeCli::parse_from(["neo-node", "--tee", "--tee-data-path", tee_path.as_str()]);
    let node_config = privacy_config("committee_key", None);
    let protocol_settings = node_config.protocol_settings();
    let system = NeoSystem::new(protocol_settings.clone(), None, None).expect("neo system");

    let err = expect_startup_err(
        maybe_enable_tee_runtime(&cli, &node_config, &protocol_settings, &system),
        "committee_key without an epoch key must fail closed",
    );
    assert!(
        err.to_string().contains("invalid [privacy] configuration"),
        "unexpected error: {err}"
    );
    assert!(
        format!("{err:#}").contains("committee_epoch_key_hex"),
        "the error chain must name the missing parameter: {err:#}"
    );

    system.shutdown().await.expect("shutdown");
}

#[tokio::test(flavor = "multi_thread")]
async fn privacy_tee_auto_never_silently_falls_back() {
    // A file (not a directory) as the TEE data path makes enclave
    // initialization fail; privacy mode must refuse to continue instead
    // of the normal --tee-auto fallback.
    let marker = tempfile::NamedTempFile::new().expect("marker file");
    let tee_path = marker.path().to_string_lossy().to_string();
    let cli = NodeCli::parse_from([
        "neo-node",
        "--tee-auto",
        "--tee-data-path",
        tee_path.as_str(),
    ]);
    let node_config = privacy_config("tee_sealed_key", None);
    let protocol_settings = node_config.protocol_settings();
    let system = NeoSystem::new(protocol_settings.clone(), None, None).expect("neo system");

    let err = expect_startup_err(
        maybe_enable_tee_runtime(&cli, &node_config, &protocol_settings, &system),
        "privacy mode must not silently fall back without the TEE",
    );
    assert!(
        err.to_string().contains("privacy mode is enabled"),
        "unexpected error: {err}"
    );

    system.shutdown().await.expect("shutdown");
}
