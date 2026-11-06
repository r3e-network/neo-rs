use futures::{SinkExt, StreamExt};
use neo_core::network::NetworkError;
use neo_core::network::p2p::{
    NetworkMessage, ProtocolMessage,
    payloads::{PingPayload, VersionPayload},
};
use neo_p2p::framed_codec::NeoMessageCodec;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio_util::codec::{FramedRead, FramedWrite};

async fn read_one_framed_frame(chunks: &[&[u8]]) -> Result<Vec<u8>, NetworkError> {
    let (reader, mut writer) = tokio::io::duplex(64);
    let chunks = chunks
        .iter()
        .map(|chunk| chunk.to_vec())
        .collect::<Vec<_>>();
    let writer_task = tokio::spawn(async move {
        for chunk in chunks {
            writer.write_all(&chunk).await.expect("write chunk");
        }
    });

    let mut framed = FramedRead::new(reader, NeoMessageCodec::default());
    let frame = framed.next().await.expect("framed reader yields a frame")?;

    writer_task.await.expect("writer task completes");
    Ok(frame)
}

#[tokio::test]
async fn framed_read_decodes_empty_payload_commands() {
    let verack = NetworkMessage::new(ProtocolMessage::Verack)
        .to_bytes(true)
        .expect("verack serializes");
    let getaddr = NetworkMessage::new(ProtocolMessage::GetAddr)
        .to_bytes(true)
        .expect("getaddr serializes");

    let first = read_one_framed_frame(&[&verack])
        .await
        .expect("verack frame decodes");
    let second = read_one_framed_frame(&[&getaddr])
        .await
        .expect("getaddr frame decodes");

    assert_eq!(first, verack);
    assert_eq!(second, getaddr);
}

#[tokio::test]
async fn framed_write_matches_network_message_bytes() {
    let ping = NetworkMessage::new(ProtocolMessage::Ping(PingPayload::create_with_nonce(
        77, 1234,
    )))
    .to_bytes(true)
    .expect("ping serializes");
    let version = NetworkMessage::new(ProtocolMessage::Version(VersionPayload::default()))
        .to_bytes(true)
        .expect("version serializes");
    let verack = NetworkMessage::new(ProtocolMessage::Verack)
        .to_bytes(true)
        .expect("verack serializes");

    for expected in [ping, version, verack] {
        let (writer, mut reader) = tokio::io::duplex(expected.len() + 8);
        let mut framed = FramedWrite::new(writer, NeoMessageCodec::default());
        framed
            .send(expected.as_slice())
            .await
            .expect("framed write succeeds");

        let mut actual = vec![0; expected.len()];
        reader
            .read_exact(&mut actual)
            .await
            .expect("read written frame");

        assert_eq!(actual, expected);
    }
}
