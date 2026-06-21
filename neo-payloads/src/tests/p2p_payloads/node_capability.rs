use super::*;

#[test]
fn disable_compression_serializes_as_csharp_capability() {
    let mut writer = BinaryWriter::new();
    Serializable::serialize(&NodeCapability::disable_compression(), &mut writer)
        .expect("serialize");
    assert_eq!(writer.into_bytes(), [0x03, 0x00]);
}

#[test]
fn disable_compression_deserializes_as_unknown_like_csharp_v310() {
    let mut reader = MemoryReader::new(&[0x03, 0x00]);
    let capability =
        <NodeCapability as Serializable>::deserialize(&mut reader).expect("deserialize");

    assert_eq!(
        capability,
        NodeCapability::Unknown {
            ty: NodeCapabilityType::DisableCompression,
            data: Vec::new(),
        },
        "C# NodeCapability.DeserializeFrom falls through to UnknownCapability for type 0x03"
    );
}

#[test]
fn disable_compression_unknown_payload_accepts_var_bytes_like_csharp() {
    let mut reader = MemoryReader::new(&[0x03, 0x01, 0xAA]);
    let capability =
        <NodeCapability as Serializable>::deserialize(&mut reader).expect("deserialize");

    assert_eq!(
        capability,
        NodeCapability::Unknown {
            ty: NodeCapabilityType::DisableCompression,
            data: vec![0xAA],
        }
    );
}

#[test]
fn duplicate_disable_compression_entries_are_filtered_as_unknown_like_csharp() {
    let mut reader = MemoryReader::new(&[0x02, 0x03, 0x00, 0x03, 0x00]);
    let capabilities = NodeCapabilities::deserialize_node_capabilities(&mut reader, 32)
        .expect("duplicates of UnknownCapability are ignored by C#");

    assert_eq!(capabilities.len(), 2);
    assert!(capabilities.iter().all(|capability| {
        matches!(
            capability,
            NodeCapability::Unknown {
                ty: NodeCapabilityType::DisableCompression,
                data
            } if data.is_empty()
        )
    }));
}
