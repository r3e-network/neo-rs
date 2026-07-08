use super::*;

/// The `VersionPayload` wire is exactly C# `VersionPayload.Serialize`:
/// `Network|Version|Timestamp|Nonce|UserAgent|Capabilities` with no top-level
/// `StartHeight`. The height travels inside the FullNode capability.
///
/// Regression: a phantom top-level `start_height` u32 injected four bytes that
/// misaligned the capability var-int and broke handshakes with real Neo nodes.
/// Symmetric Rust round-trips never caught it.
#[test]
fn version_wire_has_no_top_level_start_height_like_csharp() {
    let payload = VersionPayload {
        network: 0x334F_454E,
        version: 0,
        timestamp: 0x1122_3344,
        nonce: 0x5566_7788,
        user_agent: "/Neo:3.6.0/".to_string(),
        // Convenience field only; it must not appear on the wire.
        start_height: 999,
        capabilities: vec![NodeCapability::FullNode { start_height: 42 }],
    };
    let mut writer = BinaryWriter::new();
    Serializable::serialize(&payload, &mut writer).expect("serialize");
    let bytes = writer.into_bytes();

    let ua = payload.user_agent.as_bytes();
    let mut expected = Vec::new();
    expected.extend_from_slice(&payload.network.to_le_bytes());
    expected.extend_from_slice(&payload.version.to_le_bytes());
    expected.extend_from_slice(&payload.timestamp.to_le_bytes());
    expected.extend_from_slice(&payload.nonce.to_le_bytes());
    expected.push(ua.len() as u8);
    expected.extend_from_slice(ua);
    expected.push(0x01);
    expected.push(0x10);
    expected.extend_from_slice(&42u32.to_le_bytes());
    assert_eq!(
        bytes, expected,
        "VersionPayload must not serialize a top-level StartHeight"
    );

    assert_eq!(bytes[16 + 1 + ua.len()], 0x01);

    let decoded =
        <VersionPayload as Serializable>::deserialize(&mut MemoryReader::new(&bytes)).unwrap();
    assert_eq!(decoded.start_height, 42);
    assert_eq!(decoded.capabilities.len(), 1);
    assert_eq!(decoded.user_agent, payload.user_agent);
}
