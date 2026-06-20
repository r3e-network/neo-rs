use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn get_connection_count_defaults_to_zero() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerNode::register_handlers();
    let handler = find_handler(&handlers, "getconnectioncount");

    let result = (handler.callback())(&server, &[]).expect("get connection count");
    assert_eq!(result.as_u64().unwrap_or_default(), 0);
}
