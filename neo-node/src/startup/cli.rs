//! CLI argument override application.

use crate::cli::NodeCli;
use crate::config::NodeConfig;
use zeroize::Zeroizing;

pub(crate) fn apply_cli_overrides(cli: &NodeCli, node_config: &mut NodeConfig) {
    if let Some(path) = &cli.storage {
        node_config.storage.path = Some(path.to_string_lossy().to_string());
   }
    if let Some(magic) = cli.network_magic {
        node_config.network.network_magic = Some(magic);
   }
    if let Some(port) = cli.listen_port {
        node_config.p2p.listen_port = Some(port);
   }
    if !cli.seed_nodes.is_empty() {
        node_config.p2p.seed_nodes = cli.seed_nodes.clone();
   }
    if let Some(max_conn) = cli.max_connections {
        node_config.p2p.max_connections = Some(max_conn);
   }
    if let Some(min_conn) = cli.min_connections {
        node_config.p2p.min_desired_connections = Some(min_conn);
   }
    if let Some(max_per_address) = cli.max_connections_per_address {
        node_config.p2p.max_connections_per_address = Some(max_per_address);
   }
    if let Some(limit) = cli.broadcast_history_limit {
        node_config.p2p.broadcast_history_limit = Some(limit);
   }
    if cli.disable_compression {
        node_config.p2p.enable_compression = Some(false);
   }
    if let Some(seconds) = cli.block_time {
        node_config.blockchain.block_time = Some(seconds);
   }
    if let Some(backend) = &cli.backend {
        node_config.storage.backend = Some(backend.clone());
   }
    if let Some(bind) = &cli.rpc_bind {
        node_config.rpc.bind_address = Some(bind.clone());
   }
    if let Some(port) = cli.rpc_port {
        node_config.rpc.port = Some(port);
   }
    if cli.rpc_disable_cors {
        node_config.rpc.cors_enabled = Some(false);
   }
    if let Some(user) = &cli.rpc_user {
        node_config.rpc.rpc_user = Some(Zeroizing::new(user.clone()));
   }
    if let Some(pass) = &cli.rpc_pass {
        node_config.rpc.rpc_pass = Some(Zeroizing::new(pass.clone()));
   }
    if let Some(cert) = &cli.rpc_tls_cert {
        node_config.rpc.tls_cert_file = Some(cert.clone());
   }
    if let Some(cert_pass) = &cli.rpc_tls_cert_password {
        node_config.rpc.tls_cert_password = Some(Zeroizing::new(cert_pass.clone()));
   }
    if let Some(path) = &cli.logging_path {
        node_config.logging.file_path = Some(path.clone());
   }
    if let Some(level) = &cli.logging_level {
        node_config.logging.level = Some(level.clone());
   }
    if let Some(format) = &cli.logging_format {
        node_config.logging.format = Some(format.clone());
   }
    if cli.storage_read_only {
        node_config.storage.read_only = Some(true);
   }
    if !cli.rpc_allow_origins.is_empty() {
        node_config.rpc.allow_origins = cli.rpc_allow_origins.clone();
   }
    if !cli.rpc_disabled_methods.is_empty() {
        node_config.rpc.disabled_methods = cli.rpc_disabled_methods.clone();
   }
    if cli.rpc_hardened {
        node_config.rpc.cors_enabled = Some(false);
        node_config.rpc.auth_enabled = true;
        node_config.rpc.allow_origins.clear();
        let mut disabled = node_config.rpc.disabled_methods.clone();
        if !disabled
            .iter()
            .any(|m| m.eq_ignore_ascii_case("openwallet"))
        {
            disabled.push("openwallet".to_string());
       }
        if !disabled
            .iter()
            .any(|m| m.eq_ignore_ascii_case("listplugins"))
        {
            disabled.push("listplugins".to_string());
       }
        node_config.rpc.disabled_methods = disabled;
   }
}
