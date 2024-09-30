// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use std::io::Read;
use std::net::TcpStream;
use std::time::Duration;

use neo_base::encoding::bin::*;
use neo_core::payload::P2pMessage;
use tokio_util::bytes::BytesMut;
use tokio_util::codec::Decoder;

use crate::*;

#[test]
fn test_message_handle() {
    let config = P2pConfig {
        seeds: vec![],
        listen: "127.0.0.1:10231".into(),
        ping_interval: Duration::from_secs(1),
        ping_timeout: Duration::from_secs(3),
        ..Default::default()
    };
    let service: SocketAddr = config.listen.parse().unwrap();

    let local = LocalNode::new(config);
    let handle = MessageHandleV2::new(local.p2p_config().clone(), local.net_handles());

    let node = local.run(handle);
    std::thread::sleep(Duration::from_millis(200));

    let mut stream = TcpStream::connect(service).expect("`connect` should be ok");
    std::thread::sleep(Duration::from_millis(200));

    let discovery = node.discovery();
    let count = discovery.lock().unwrap().connected_peers().count();
    assert_eq!(count, 1);

    let mut buf = BytesMut::zeroed(1024);
    let n = stream.read(buf.as_mut()).expect("`read` should be ok");
    buf.truncate(n);

    let mut decoder = MessageDecoder;
    let message = decoder
        .decode(&mut buf)
        .expect("`decode` should be ok")
        .expect("message should be Some(Bytes)");

    let mut buf = RefBuffer::from(message.as_bytes());
    let message: P2pMessage = BinDecoder::decode_bin(&mut buf).expect("`decode_bin` should be ok");
    // println!("message {:?}", &message);

    let P2pMessage::Version(version) = message else {
        panic!("should be Version")
    };
    assert_eq!(version.version, 0);
    assert_eq!(version.network, Network::DevNet.as_magic());

    let mut buf = BytesMut::zeroed(1024);
    let n = stream.read(buf.as_mut()).expect("`read` should be ok");
    buf.truncate(n);

    let message = decoder
        .decode(&mut buf)
        .expect("`decode` should be ok")
        .expect("message should be Some(Bytes)");

    let mut buf = RefBuffer::from(message.as_bytes());
    let message: P2pMessage = BinDecoder::decode_bin(&mut buf).expect("`decode_bin` should be ok");

    let P2pMessage::Ping(ping) = message else {
        panic!("should be Ping")
    };
    assert_eq!(ping.nonce, version.nonce);

    drop(node);
    std::thread::sleep(Duration::from_millis(500));
}
