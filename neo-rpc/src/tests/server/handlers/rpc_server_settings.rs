use super::*;
use serde_json::Value;
use std::fs;
use std::net::{IpAddr, Ipv4Addr};
use std::path::PathBuf;

#[test]
fn rpc_server_config_loads_csharp_settings() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let config_path = manifest_dir.join("../neo_csharp/node/plugins/RpcServer/RpcServer.json");
    if !config_path.exists() {
        eprintln!(
            "SKIP: neo_csharp submodule not initialized (missing {})",
            config_path.display()
        );
        return;
    }
    let raw = fs::read_to_string(&config_path).expect("read rpc server config");
    let json: Value = serde_json::from_str(&raw).expect("parse rpc server config");
    let servers = json["PluginConfiguration"]["Servers"]
        .as_array()
        .expect("servers array");
    let server = servers.first().expect("server entry");

    let config: RpcServerConfig =
        serde_json::from_value(server.clone()).expect("deserialize config");

    assert_eq!(config.network, 860_833_102);
    assert_eq!(config.port, 10332);
    assert_eq!(config.bind_address, IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
    assert_eq!(config.ssl_cert, "");
    assert_eq!(config.ssl_cert_password, "");
    assert!(config.trusted_authorities.is_empty());
    assert_eq!(config.rpc_user, "");
    assert_eq!(config.rpc_pass, "");
    assert!(config.enable_cors);
    assert!(config.allow_origins.is_empty());
    assert_eq!(config.keep_alive_timeout, 60);
    assert_eq!(config.request_headers_timeout, 15);
    assert_eq!(config.max_gas_invoke, 2_000_000_000);
    assert_eq!(config.max_fee, 10_000_000);
    assert_eq!(config.max_concurrent_connections, 40);
    assert_eq!(config.max_request_body_size, 5 * 1024 * 1024);
    assert_eq!(config.max_iterator_result_items, 100);
    assert_eq!(config.max_stack_size, 65_535);
    assert_eq!(config.disabled_methods, vec!["openwallet"]);
    assert!(!config.session_enabled);
    assert_eq!(config.session_expiration_time, 60);
    assert_eq!(config.find_storage_page_size, 50);
}
