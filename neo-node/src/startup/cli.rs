//! CLI argument override application.

use crate::cli::NodeCli;
use crate::config::NodeConfig;
use zeroize::Zeroizing;

/// RPC methods that `--rpc-hardened` adds to `rpc.disabled_methods`.
///
/// The RPC server refuses a request whose whole method name equals a disabled
/// name, ignoring ASCII case; there is no prefix or pattern matching. An entry
/// that is not the name of a registered handler therefore disables nothing.
///
/// `calculatenetworkfee` is the one protected method left out on purpose: it is
/// a read-only estimate that works without an opened wallet.
const HARDENED_DISABLED_METHODS: &[&str] = &[
    "openwallet",
    "closewallet",
    "listplugins",
    "dumpprivkey",
    "importprivkey",
    "getnewaddress",
    "getwalletbalance",
    "getwalletunclaimedgas",
    "listaddress",
    "sendfrom",
    "sendtoaddress",
    "sendmany",
    "canceltransaction",
    "sendrawtransaction",
    "startconsensus",
];

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
        node_config.p2p.seed_nodes.clone_from(&cli.seed_nodes);
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
        node_config.blockchain.block_time = Some(seconds.saturating_mul(1_000));
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
        node_config
            .rpc
            .allow_origins
            .clone_from(&cli.rpc_allow_origins);
    }
    if !cli.rpc_disabled_methods.is_empty() {
        node_config
            .rpc
            .disabled_methods
            .clone_from(&cli.rpc_disabled_methods);
    }
    if cli.rpc_hardened {
        node_config.rpc.cors_enabled = Some(false);
        node_config.rpc.auth_enabled = true;
        node_config.rpc.allow_origins.clear();
        let mut disabled = node_config.rpc.disabled_methods.clone();
        for &method in HARDENED_DISABLED_METHODS {
            if !disabled.iter().any(|m| m.eq_ignore_ascii_case(method)) {
                if method == "sendrawtransaction" {
                    tracing::warn!(
                        "RPC hardened mode: sendrawtransaction is disabled. \
                        Clients attempting to send transactions will receive \
                        -600 Access denied."
                    );
                }
                disabled.push(method.to_string());
            }
        }
        node_config.rpc.disabled_methods = disabled;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc_consensus::RpcServerConsensus;
    use clap::Parser;
    use neo_rpc::server::{
        RpcServerApplicationLogs, RpcServerBlockchain, RpcServerNode, RpcServerOracle,
        RpcServerPrivacy, RpcServerSmartContract, RpcServerState, RpcServerTokensTracker,
        RpcServerUtilities, RpcServerWallet,
    };
    use std::collections::HashSet;

    /// Names of every handler the node can register on its RPC server: the sets
    /// of `start_rpc_server_if_enabled` and `install_privacy_rpc_endpoint`, plus
    /// the consensus handlers registered in `run`. Keyed like the server does.
    fn registrable_rpc_methods() -> HashSet<String> {
        [
            RpcServerNode::register_handlers(),
            RpcServerBlockchain::register_handlers(),
            RpcServerUtilities::register_handlers(),
            RpcServerSmartContract::register_handlers(),
            RpcServerWallet::register_handlers(),
            RpcServerApplicationLogs::register_handlers(),
            RpcServerState::register_handlers(),
            RpcServerTokensTracker::register_handlers(),
            RpcServerOracle::register_handlers(),
            RpcServerPrivacy::register_handlers(),
            RpcServerConsensus::register_handlers(),
        ]
        .into_iter()
        .flatten()
        .map(|handler| handler.descriptor().name.to_ascii_lowercase())
        .collect()
    }

    #[test]
    fn hardened_disabled_methods_are_registrable_rpc_methods() {
        let registrable = registrable_rpc_methods();
        let unknown: Vec<&str> = HARDENED_DISABLED_METHODS
            .iter()
            .copied()
            .filter(|name| !registrable.contains(&name.to_ascii_lowercase()))
            .collect();

        assert!(
            unknown.is_empty(),
            "--rpc-hardened disables names that no RPC handler is registered under: {unknown:?}"
        );
    }

    #[test]
    fn rpc_hardened_appends_its_methods_to_the_configured_ones() {
        let cli = NodeCli::parse_from([
            "neo-node",
            "--rpc-disabled-methods",
            "SendRawTransaction,getpeers",
            "--rpc-hardened",
        ]);
        let mut node_config = NodeConfig::default();

        apply_cli_overrides(&cli, &mut node_config);

        let mut expected = vec!["SendRawTransaction", "getpeers"];
        expected.extend(
            HARDENED_DISABLED_METHODS
                .iter()
                .copied()
                .filter(|name| *name != "sendrawtransaction"),
        );
        assert_eq!(node_config.rpc.disabled_methods, expected);
        assert_eq!(node_config.rpc.cors_enabled, Some(false));
        assert!(node_config.rpc.auth_enabled);
        assert!(node_config.rpc.allow_origins.is_empty());
    }
}
