use super::*;
use neo_payloads::ping_payload::PingPayload;

#[test]
fn message_round_trip_uncompressed_ping() {
    let ping = PingPayload::create(42);
    let msg = Message::create(MessageCommand::Ping, Some(&ping), false).expect("create");
    let bytes = msg.to_bytes().expect("encode");
    let decoded = Message::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.command, MessageCommand::Ping);
    assert_eq!(decoded.payload_raw, msg.payload_raw);
    assert_eq!(decoded.flags, MessageFlags::NONE);
}

#[test]
fn message_compresses_large_payload_when_allowed() {
    let payload = vec![0xABu8; COMPRESSION_MIN_SIZE + 100];
    let msg = Message::from_payload_bytes(MessageCommand::FilterAdd, payload.clone(), true)
        .expect("create");
    assert_eq!(msg.flags, MessageFlags::COMPRESSED);
    assert!(msg.payload_compressed.len() < payload.len());

    let bytes = msg.to_bytes().expect("encode");
    let decoded = Message::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.payload_raw, payload);
}

/// C# `Message.Create` (Message.cs:100) gates on `> CompressionMinSize`
/// (strictly): a payload of exactly 128 bytes is never compressed.
#[test]
fn message_at_min_size_boundary_is_not_compressed() {
    let payload = vec![0xABu8; COMPRESSION_MIN_SIZE];
    let msg = Message::from_payload_bytes(MessageCommand::FilterAdd, payload.clone(), true)
        .expect("create");
    assert_eq!(
        msg.flags,
        MessageFlags::NONE,
        "len == 128 must not compress"
    );
    assert_eq!(msg.payload_compressed, payload);
}

/// C# requires LZ4 to save more than `CompressionThreshold` (64) bytes;
/// an incompressible payload above the min size is sent raw.
#[test]
fn message_incompressible_payload_sent_raw() {
    // High-entropy bytes resist LZ4, so the compressed form does not beat
    // `raw.len() - 64` and the message stays uncompressed.
    let payload: Vec<u8> = (0..160u32)
        .map(|i| (i.wrapping_mul(2_654_435_761) >> 8) as u8)
        .collect();
    let msg = Message::from_payload_bytes(MessageCommand::FilterAdd, payload.clone(), true)
        .expect("create");
    assert_eq!(msg.flags, MessageFlags::NONE);
    assert_eq!(msg.payload_compressed, payload);
}

#[test]
fn message_rejects_oversized_payload() {
    let payload = vec![0u8; PAYLOAD_MAX_SIZE + 1];
    let err = Message::from_payload_bytes(MessageCommand::Block, payload, false)
        .expect_err("must reject");
    assert!(matches!(err, WireError::PayloadTooLarge(_, _)));
}

#[test]
fn message_preserves_unknown_flag_bits_like_csharp() {
    // C# Message.Deserialize casts the raw byte to the [Flags] enum and
    // then checks HasFlag(Compressed); reserved bits do not reject the
    // frame.
    let bytes = [0x80, MessageCommand::Verack.to_byte(), 0x00];
    let decoded = Message::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.flags.to_byte(), 0x80);
    assert!(!decoded.flags.is_compressed());
    assert!(decoded.payload_raw.is_empty());
}

#[test]
fn message_accepts_empty_payload_with_compressed_flag_like_csharp() {
    // C# Message.DecompressPayload returns immediately when
    // _payloadCompressed.Length == 0, even if the compressed bit is set.
    let bytes = [
        MessageFlags::COMPRESSED.to_byte(),
        MessageCommand::Verack.to_byte(),
        0x00,
    ];

    let decoded = Message::from_bytes(&bytes).expect("decode");
    assert_eq!(decoded.flags, MessageFlags::COMPRESSED);
    assert!(decoded.payload_compressed.is_empty());
    assert!(decoded.payload_raw.is_empty());
}
