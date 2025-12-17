use neo_core::neo_io::{MemoryReader, Serializable, SerializableExt};
use neo_core::network::p2p::capabilities::{NodeCapability, NodeCapabilityType};
use neo_core::network::p2p::payloads::{AddrPayload, NetworkAddressWithTime, VersionPayload};
use std::net::{IpAddr, Ipv4Addr};

#[test]
fn csharp_ut_version_payload_size_and_roundtrip() {
    let empty = VersionPayload {
        network: 123,
        version: neo_core::network::p2p::payloads::version_payload::PROTOCOL_VERSION,
        timestamp: 456,
        nonce: 789,
        user_agent: "neo3".to_string(),
        allow_compression: true,
        capabilities: vec![],
    };
    assert_eq!(empty.size(), 22);

    let with_cap = VersionPayload {
        network: 123,
        version: neo_core::network::p2p::payloads::version_payload::PROTOCOL_VERSION,
        timestamp: 456,
        nonce: 789,
        user_agent: "neo3".to_string(),
        allow_compression: true,
        capabilities: vec![NodeCapability::tcp_server(22)],
    };
    assert_eq!(with_cap.size(), 25);

    let bytes = with_cap.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <VersionPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(with_cap, clone);
}

#[test]
fn csharp_ut_version_payload_duplicate_capability_rejected() {
    let payload = VersionPayload {
        network: 123,
        version: neo_core::network::p2p::payloads::version_payload::PROTOCOL_VERSION,
        timestamp: 456,
        nonce: 789,
        user_agent: "neo3".to_string(),
        allow_compression: true,
        capabilities: vec![
            NodeCapability::tcp_server(22),
            NodeCapability::tcp_server(22),
        ],
    };
    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<VersionPayload as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_version_payload_allows_unknown_capabilities() {
    let unknown_a = NodeCapability::unknown_from_byte(0xFE, vec![]).expect("unknown");
    let unknown_b = NodeCapability::unknown_from_byte(0xFD, vec![0x00, 0x00]).expect("unknown");

    let payload = VersionPayload {
        network: 123,
        version: neo_core::network::p2p::payloads::version_payload::PROTOCOL_VERSION,
        timestamp: 456,
        nonce: 789,
        user_agent: "neo3".to_string(),
        allow_compression: true,
        capabilities: vec![
            NodeCapability::tcp_server(22),
            unknown_a,
            unknown_b,
            NodeCapability::full_node(1),
        ],
    };

    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <VersionPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(clone.capabilities.len(), 4);
    assert_eq!(
        clone
            .capabilities
            .iter()
            .filter(|c| matches!(c, NodeCapability::Unknown { .. }))
            .count(),
        2
    );
}

#[test]
fn csharp_ut_network_address_with_time_size_and_endpoint() {
    let addr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let empty = NetworkAddressWithTime::new(1, addr, vec![]);
    assert_eq!(empty.size(), 21);
    assert_eq!(empty.endpoint().map(|e| e.port()).unwrap_or(0), 0);

    let with_cap = NetworkAddressWithTime::new(1, addr, vec![NodeCapability::tcp_server(22)]);
    assert_eq!(with_cap.size(), 24);
    assert_eq!(with_cap.endpoint().map(|e| e.port()).unwrap_or(0), 22);
}

#[test]
fn csharp_ut_network_address_with_time_roundtrip_and_duplicate_known_rejected() {
    let addr = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let ext0 = NodeCapability::unknown(NodeCapabilityType::Extension0, vec![]).expect("unknown");

    let test = NetworkAddressWithTime::new(
        1,
        addr,
        vec![NodeCapability::tcp_server(22), ext0.clone(), ext0.clone()],
    );
    let bytes = test.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone =
        <NetworkAddressWithTime as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(test, clone);

    let dup_known = NetworkAddressWithTime::new(
        1,
        addr,
        vec![
            NodeCapability::tcp_server(22),
            NodeCapability::tcp_server(22),
        ],
    );
    let bytes = dup_known.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<NetworkAddressWithTime as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_addr_payload_size_roundtrip_and_empty_rejected() {
    let empty = AddrPayload::create(vec![]);
    assert_eq!(empty.size(), 1);

    let addr = NetworkAddressWithTime::new(1, IpAddr::V4(Ipv4Addr::UNSPECIFIED), vec![]);
    let payload = AddrPayload::create(vec![addr]);
    assert_eq!(payload.size(), 22);

    let bytes = payload.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    let clone = <AddrPayload as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(payload, clone);

    let bytes = empty.to_array().expect("serialize");
    let mut reader = MemoryReader::new(&bytes);
    assert!(<AddrPayload as Serializable>::deserialize(&mut reader).is_err());
}
