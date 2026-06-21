use super::*;
use crate::MessageCommand;
use neo_payloads::ping_payload::PingPayload;

#[test]
fn codec_encodes_and_decodes_ping_message() {
    let mut codec = MessageCodec::new();
    let ping = PingPayload::create(99);
    let msg = Message::create(MessageCommand::Ping, Some(&ping), false).expect("create");

    let mut buf = BytesMut::new();
    codec.encode(msg.clone(), &mut buf).expect("encode");

    let decoded = codec.decode(&mut buf).expect("decode").expect("frame");
    assert_eq!(decoded.command, MessageCommand::Ping);
    assert_eq!(decoded.payload_raw, msg.payload_raw);
    assert!(buf.is_empty());
}

#[test]
fn codec_returns_none_for_partial_frame() {
    let mut codec = MessageCodec::new();
    let mut buf = BytesMut::from(&[0x00u8][..]);
    assert!(codec.decode(&mut buf).expect("decode").is_none());
}

#[test]
fn codec_decodes_two_frames_from_one_buffer() {
    let mut codec = MessageCodec::new();
    let msg1 = Message::create(MessageCommand::Ping, Some(&PingPayload::create(1)), false)
        .expect("create");
    let msg2 = Message::create(MessageCommand::Ping, Some(&PingPayload::create(2)), false)
        .expect("create");

    let mut buf = BytesMut::new();
    codec.encode(msg1.clone(), &mut buf).expect("encode 1");
    codec.encode(msg2.clone(), &mut buf).expect("encode 2");

    let d1 = codec.decode(&mut buf).expect("decode 1").expect("frame 1");
    let d2 = codec.decode(&mut buf).expect("decode 2").expect("frame 2");
    assert!(buf.is_empty());
    assert_eq!(d1.payload_raw, msg1.payload_raw);
    assert_eq!(d2.payload_raw, msg2.payload_raw);
}
