use super::*;
use neo_protocol::ProtocolSettings;
use neo_system::NeoSystem;
use neo_system::state_service::{StateServiceSettings, UnhandledExceptionPolicy};

fn make_server() -> (Arc<NeoSystem>, RpcServer, tempfile::TempDir) {
    let temp = tempfile::tempdir().expect("temporary state directory");
    let settings = ProtocolSettings::default();
    let state_settings = StateServiceSettings {
        full_state: true,
        path: temp.path().to_string_lossy().into_owned(),
        network: settings.network,
        auto_verify: false,
        max_find_result_items: 100,
        exception_policy: UnhandledExceptionPolicy::StopPlugin,
    };
    let system = NeoSystem::new_with_state_service(settings, None, None, Some(state_settings))
        .expect("state service should initialise");
    let mut server = RpcServer::new(system.clone(), Default::default());
    server.register_handlers(RpcServerState::register_handlers());
    (system, server, temp)
}

#[test]
fn registers_only_canonically_supported_state_methods() {
    let handlers = RpcServerState::register_handlers();
    let names: Vec<&str> = handlers
        .iter()
        .map(|handler| handler.descriptor().name.as_str())
        .collect();
    assert_eq!(names, ["getstateroot", "getstateheight"]);
}

#[tokio::test]
async fn reports_real_local_and_validated_heights() {
    let (system, server, _temp) = make_server();
    let height = RpcServerState::get_state_height(&server, &[]).expect("state height");
    assert_eq!(height["localrootindex"], 0);
    assert_eq!(height["validatedrootindex"], 0);
    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn missing_state_root_is_not_fabricated() {
    let (system, server, _temp) = make_server();
    let error = RpcServerState::get_state_root(&server, &[Value::from(9_999)])
        .expect_err("unknown root must fail");
    assert_eq!(error.code(), RpcError::unknown_state_root().code());
    system.shutdown().await.expect("shutdown");
}
