use super::*;

#[test]
fn capabilities_omit_tcp_server_until_listening() {
    let identity = LocalIdentity::new(860_833_102, 7, "/neo-rs:test/".to_string(), true);
    let caps = identity.capabilities();
    assert!(
        caps.iter()
            .all(|c| !matches!(c, NodeCapability::TcpServer { .. }))
    );
    assert!(
        caps.iter()
            .any(|c| matches!(c, NodeCapability::FullNode { start_height: 0 }))
    );
    assert!(
        caps.iter()
            .any(|c| matches!(c, NodeCapability::ArchivalNode))
    );

    identity.set_listen_port(20333);
    let caps = identity.capabilities();
    assert!(
        caps.iter()
            .any(|c| matches!(c, NodeCapability::TcpServer { port: 20333 }))
    );
}

#[test]
fn capabilities_advertise_disable_compression_when_disabled() {
    let identity = LocalIdentity::new(1, 2, "/neo-rs:test/".to_string(), false);
    assert!(
        identity
            .capabilities()
            .iter()
            .any(|c| matches!(c, NodeCapability::DisableCompression))
    );
}

#[test]
fn version_payload_carries_identity_fields() {
    let identity = LocalIdentity::new(894_710_606, 42, "/neo-rs:test/".to_string(), true);
    identity.set_listen_port(30333);
    let payload = identity.version_payload();
    assert_eq!(payload.network, 894_710_606);
    assert_eq!(payload.nonce, 42);
    assert_eq!(payload.user_agent, "/neo-rs:test/");
    assert!(
        payload
            .capabilities
            .iter()
            .any(|c| matches!(c, NodeCapability::TcpServer { port: 30333 }))
    );
}
