use super::NodeCapabilityType;

#[test]
fn node_capability_type_matches_neo_values() {
    assert_eq!(NodeCapabilityType::TcpServer.to_byte(), 0x01);
    assert_eq!(NodeCapabilityType::WsServer.to_byte(), 0x02);
    assert_eq!(NodeCapabilityType::DisableCompression.to_byte(), 0x03);
    assert_eq!(NodeCapabilityType::FullNode.to_byte(), 0x10);
    assert_eq!(NodeCapabilityType::ArchivalNode.to_byte(), 0x11);
    assert_eq!(NodeCapabilityType::Extension0.to_byte(), 0xF0);

    assert_eq!(
        NodeCapabilityType::from_byte(0x99),
        NodeCapabilityType::Unknown(0x99)
    );
}

#[test]
fn node_capability_type_preserves_unknown_bytes() {
    let unknown = NodeCapabilityType::from_byte(0x99);
    assert_eq!(unknown.to_byte(), 0x99);
    assert_eq!(unknown.as_byte(), 0x99);
    assert_eq!(unknown.as_str(), "Unknown");
    assert!(!unknown.is_known());
    assert_eq!(unknown.to_string(), "Unknown(0x99)");
}

#[test]
fn node_capability_type_serde_shape_matches_derived_enum() {
    let serialized = serde_json::to_string(&NodeCapabilityType::FullNode).unwrap();
    assert_eq!(serialized, "\"FullNode\"");

    let unknown = NodeCapabilityType::Unknown(0x99);
    let serialized_unknown = serde_json::to_string(&unknown).unwrap();
    assert_eq!(serialized_unknown, "{\"Unknown\":153}");

    let deserialized: NodeCapabilityType = serde_json::from_str(&serialized_unknown).unwrap();
    assert_eq!(deserialized, unknown);
}
