use neo_core::neo_io::{MemoryReader, Serializable, SerializableExt};
use neo_core::network::p2p::capabilities::{NodeCapability, NodeCapabilityType};

#[test]
fn csharp_ut_capability_size_get() {
    assert_eq!(NodeCapability::archival_node().size(), 2);
    assert_eq!(NodeCapability::disable_compression().size(), 2);
    assert_eq!(NodeCapability::full_node(1).size(), 5);
    assert_eq!(NodeCapability::tcp_server(1).size(), 3);
    assert_eq!(NodeCapability::ws_server(2).size(), 3);
}

#[test]
fn csharp_ut_archival_node_capability_deserialize_and_serialize() {
    let cap = NodeCapability::archival_node();
    let mut buf = cap.to_array().expect("serialize");

    let mut reader = MemoryReader::new(&buf);
    let clone = <NodeCapability as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(cap, clone);

    // Marker byte must be zero.
    buf[1] = 0x01;
    let mut reader = MemoryReader::new(&buf);
    assert!(<NodeCapability as Serializable>::deserialize(&mut reader).is_err());
}

#[test]
fn csharp_ut_server_capability_deserialize_and_serialize() {
    let cap = NodeCapability::ws_server(2);
    let mut buf = cap.to_array().expect("serialize");

    let mut reader = MemoryReader::new(&buf);
    let clone = <NodeCapability as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert_eq!(cap, clone);

    // Mutate type to an unknown capability identifier without adjusting payload encoding.
    // This should fail because unknown capabilities use var-bytes encoding.
    buf[0] = 0xFF;
    let mut reader = MemoryReader::new(&buf);
    assert!(<NodeCapability as Serializable>::deserialize(&mut reader).is_err());

    // Known capability types must not be constructed via unknown().
    assert!(NodeCapability::unknown(NodeCapabilityType::TcpServer, vec![]).is_err());
    assert!(NodeCapability::unknown(NodeCapabilityType::FullNode, vec![0]).is_err());
}

#[test]
fn csharp_ut_unknown_capability_deserialize_unknown_roundtrip() {
    let buf = [0xff, 0x03, 0x01, 0x02, 0x03];
    let mut reader = MemoryReader::new(&buf);
    let cap = <NodeCapability as Serializable>::deserialize(&mut reader).expect("deserialize");
    assert!(matches!(cap, NodeCapability::Unknown { .. }));
    assert_eq!(cap.to_array().expect("serialize"), buf);
}

