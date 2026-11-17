//
// tests.rs - Unit tests for local node
//

#![allow(clippy::module_inception)]

use super::state::LocalNode;
use super::*;

mod tests {
    use super::*;
    use crate::p2p::payloads::extensible_payload::ExtensiblePayload;
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;

    #[test]
    fn tcp_server_capability_tracks_channels_config() {
        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings, 10333, "/agent".to_string());

        // No TCP endpoint configured should clear the advertised server capability.
        node.apply_channels_config(&ChannelsConfig::default());
        assert_eq!(node.port(), 0);
        let payload = node.version_payload();
        assert!(
            payload
                .capabilities
                .iter()
                .all(|cap| !matches!(cap, NodeCapability::TcpServer { .. }))
        );

        // Enabling a TCP endpoint should advertise the matching capability and port.
        let config = ChannelsConfig {
            tcp: Some("127.0.0.1:20333".parse().expect("endpoint")),
            ..Default::default()
        };
        node.apply_channels_config(&config);
        assert_eq!(node.port(), 20333);

        let payload = node.version_payload();
        assert!(
            payload
                .capabilities
                .iter()
                .any(|cap| { matches!(cap, NodeCapability::TcpServer { port } if *port == 20333) })
        );
    }

    #[test]
    fn compression_capability_respects_configuration() {
        use crate::p2p::payloads::NodeCapability;

        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings, 10333, "/agent".to_string());

        let mut config = ChannelsConfig::default();
        node.apply_channels_config(&config);

        // Compression is allowed by default (no DisableCompression capability)
        let payload = node.version_payload();
        assert!(
            !payload
                .capabilities
                .iter()
                .any(|cap| matches!(cap, NodeCapability::DisableCompression))
        );

        config.enable_compression = false;
        node.apply_channels_config(&config);
        let payload = node.version_payload();
        assert!(
            payload
                .capabilities
                .iter()
                .any(|cap| matches!(cap, NodeCapability::DisableCompression))
        );

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

        let version =
            VersionPayload::create(settings.network, 12345, "/peer".to_string(), Vec::new());
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
    fn pending_inbound_slots_count_toward_connection_caps() {
        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings, 10333, "/agent".to_string());

        let config = ChannelsConfig {
            max_connections: 2,
            max_connections_per_address: 1,
            ..Default::default()
        };
        node.apply_channels_config(&config);

        let first: SocketAddr = "203.0.113.5:20301".parse().unwrap();
        let second: SocketAddr = "203.0.113.5:20302".parse().unwrap();
        let third: SocketAddr = "203.0.113.6:20303".parse().unwrap();
        let fourth: SocketAddr = "198.51.100.7:20400".parse().unwrap();

        // A connection that has not sent `version` yet still occupies a slot.
        assert!(node.validate_inbound_connection(&first).is_ok());
        assert_eq!(node.try_reserve_inbound_slot("actor-a", first), Ok(()));
        assert_eq!(node.pending_inbound_count(), 1);
        assert_eq!(node.occupied_connection_slots(), 1);

        // The per-address cap applies before the handshake completes.
        assert_eq!(
            node.try_reserve_inbound_slot("actor-b", second),
            Err("connection limit per address reached")
        );
        assert_eq!(
            node.validate_inbound_connection(&second),
            Err("connection limit per address reached")
        );

        // A different address fits the per-address cap but consumes the global cap.
        assert_eq!(node.try_reserve_inbound_slot("actor-c", third), Ok(()));
        assert_eq!(node.pending_inbound_count(), 2);
        assert_eq!(
            node.validate_inbound_connection(&fourth),
            Err("connection limit reached")
        );

        // Reservations are tracked by actor name until the actor path is known,
        // then released when the connection tears down.
        assert!(node.rename_inbound_slot("actor-a", "/user/remote-a"));
        assert!(node.release_inbound_slot("/user/remote-a"));
        assert!(!node.release_inbound_slot("/user/remote-a"));
        assert_eq!(node.pending_inbound_count(), 1);
    }

    #[test]
    fn ban_peer_rejects_inbound_until_unbanned() {
        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings, 10333, "/agent".to_string());
        let remote: SocketAddr = "198.51.100.10:20333".parse().unwrap();
        let ip = remote.ip();

        assert!(node.validate_inbound_connection(&remote).is_ok());

        node.ban_peer(ip, Duration::from_secs(60), "test ban");

        assert_eq!(
            node.validate_inbound_connection(&remote),
            Err("peer is banned")
        );
        assert!(node.unban_peer(&ip));
        assert!(node.validate_inbound_connection(&remote).is_ok());
    }

    #[test]
    fn expired_ban_is_not_active_and_cleanup_removes_it() {
        let settings = Arc::new(ProtocolSettings::default());
        let node = LocalNode::new(settings, 10333, "/agent".to_string());
        let remote: SocketAddr = "198.51.100.11:20333".parse().unwrap();
        let ip = remote.ip();

        node.ban_peer(ip, Duration::ZERO, "expired test ban");

        assert!(!node.is_ip_banned(&ip));
        assert_eq!(node.active_ban_count(), 0);
        assert_eq!(node.cleanup_expired_bans(), 1);
        assert_eq!(node.cleanup_expired_bans(), 0);
        assert!(node.validate_inbound_connection(&remote).is_ok());
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
