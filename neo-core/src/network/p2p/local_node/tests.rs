//
// tests.rs - Unit tests for local node
//

#![allow(clippy::module_inception)]

use super::state::LocalNode;
use super::*;

mod tests {
    use super::*;
    use crate::network::p2p::payloads::extensible_payload::ExtensiblePayload;
    use std::sync::Arc;

    #[test]
    fn tcp_server_capability_tracks_channels_config() {
        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings, 10333, "/agent".to_string());

        // No TCP endpoint configured should clear the advertised server capability.
        node.apply_channels_config(&ChannelsConfig::default());
        assert_eq!(node.port(), 0);
        let payload = node.version_payload();
        assert!(payload
            .capabilities
            .iter()
            .all(|cap| !matches!(cap, NodeCapability::TcpServer { .. })));

        // Enabling a TCP endpoint should advertise the matching capability and port.
        let config = ChannelsConfig {
            tcp: Some("127.0.0.1:20333".parse().expect("endpoint")),
            ..Default::default()
        };
        node.apply_channels_config(&config);
        assert_eq!(node.port(), 20333);

        let payload = node.version_payload();
        assert!(payload
            .capabilities
            .iter()
            .any(|cap| { matches!(cap, NodeCapability::TcpServer { port } if *port == 20333) }));
    }

    #[test]
    fn compression_capability_respects_configuration() {
        use crate::network::p2p::capabilities::NodeCapability;

        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings, 10333, "/agent".to_string());

        let mut config = ChannelsConfig::default();
        node.apply_channels_config(&config);

        // Compression is allowed by default (no DisableCompression capability)
        let payload = node.version_payload();
        assert!(!payload
            .capabilities
            .iter()
            .any(|cap| matches!(cap, NodeCapability::DisableCompression)));

        config.enable_compression = false;
        node.apply_channels_config(&config);
        let payload = node.version_payload();
        assert!(payload
            .capabilities
            .iter()
            .any(|cap| matches!(cap, NodeCapability::DisableCompression)));

        let stored = node.config();
        assert_eq!(stored.enable_compression, config.enable_compression);
        assert_eq!(
            stored.min_desired_connections,
            ChannelsConfig::DEFAULT_MIN_DESIRED_CONNECTIONS
        );
        assert_eq!(
            stored.max_connections_per_address,
            ChannelsConfig::DEFAULT_MAX_CONNECTIONS_PER_ADDRESS
        );
    }

    #[test]
    fn allow_new_connection_respects_limits() {
        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings.clone(), 10333, "/agent".to_string());

        let config = ChannelsConfig {
            max_connections: 1,
            max_connections_per_address: 1,
            ..Default::default()
        };
        node.apply_channels_config(&config);

        let version = VersionPayload::create(&settings, 12345, "/peer".to_string(), Vec::new());
        let existing = RemoteNodeSnapshot {
            remote_address: "10.0.0.1:20000".parse().unwrap(),
            remote_port: 20000,
            listen_tcp_port: 20000,
            last_block_index: 0,
            version: version.version,
            services: 0,
            timestamp: 0,
        };
        node.add_peer(
            existing.remote_address,
            Some(existing.listen_tcp_port),
            existing.version,
            existing.services,
            existing.last_block_index,
        );

        let incoming = RemoteNodeSnapshot {
            remote_address: "10.0.0.2:20001".parse().unwrap(),
            remote_port: 20001,
            listen_tcp_port: 20001,
            last_block_index: 0,
            version: version.version,
            services: 0,
            timestamp: 0,
        };

        assert!(!node.allow_new_connection(&incoming, &version));
    }

    #[test]
    fn broadcast_history_is_bounded() {
        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings, 10333, "/agent".to_string());

        let mut payload = ExtensiblePayload::new();
        payload.valid_block_end = 1;
        let inventory = RelayInventory::Extensible(payload);

        let cap = ChannelsConfig::DEFAULT_BROADCAST_HISTORY_LIMIT;
        for _ in 0..(cap + 50) {
            node.record_relay(&inventory);
        }

        assert!(
            node.broadcast_history().len() <= cap,
            "broadcast history exceeded configured cap"
        );
    }

    #[test]
    fn broadcast_history_can_be_disabled() {
        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings, 10333, "/agent".to_string());

        let mut config = node.config();
        config.broadcast_history_limit = 0;
        node.apply_channels_config(&config);

        let mut payload = ExtensiblePayload::new();
        payload.valid_block_end = 1;
        let inventory = RelayInventory::Extensible(payload);

        node.record_relay(&inventory);
        assert!(node.broadcast_history().is_empty());
    }
}
