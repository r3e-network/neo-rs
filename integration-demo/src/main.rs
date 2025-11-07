use anyhow::Result;
use bytes::BytesMut;
use clap::Parser;
use neo_base::{Bytes, NeoDecode, NeoEncode, SliceReader};
use neo_base::{Hash256, NeoDecodeDerive, NeoEncodeDerive};
use neo_crypto::ecc256::{PrivateKey, PublicKey};
use neo_crypto::{Secp256r1Sign, Secp256r1Verify};
use neo_p2p::message::{
    AddressEntry, AddressPayload, Endpoint, InventoryItem, InventoryKind, InventoryPayload,
    Message, NetworkAddress, PayloadWithData,
};
use neo_p2p::{build_version_payload, NeoMessageCodec, Peer, PeerEvent};
use neo_store::{BlockRecord, Blocks, Column, HeaderRecord, Headers, MemoryStore, StoreExt};
use p256::{ecdsa::SigningKey, SecretKey};
use std::net::{IpAddr, Ipv4Addr};
use tokio_util::codec::{Decoder, Encoder};

#[derive(Parser, Debug)]
#[command(name = "integration-demo", about = "Neo N3 building blocks demo")]
struct Cli {
    /// Skip persisting header/block records in the memory store
    #[arg(long)]
    skip_store: bool,

    /// Skip signing and verifying the demo payload
    #[arg(long)]
    skip_crypto: bool,

    /// Skip the handshake and inventory simulation steps
    #[arg(long)]
    skip_handshake: bool,
}

#[derive(Clone)]
struct DemoConfig {
    store: bool,
    crypto: bool,
    handshake: bool,
    network_magic: u32,
}

impl From<Cli> for DemoConfig {
    fn from(cli: Cli) -> Self {
        Self {
            store: !cli.skip_store,
            crypto: !cli.skip_crypto,
            handshake: !cli.skip_handshake,
            network_magic: 860_833_102, // mainnet magic
        }
    }
}

impl Default for DemoConfig {
    fn default() -> Self {
        Self {
            store: true,
            crypto: true,
            handshake: true,
            network_magic: 860_833_102,
        }
    }
}

#[derive(Clone, PartialEq, Debug, NeoEncodeDerive, NeoDecodeDerive)]
struct DemoHeader {
    hash: Hash256,
    payload: Bytes,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    run_demo(DemoConfig::from(cli))
}

fn run_demo(config: DemoConfig) -> Result<()> {
    println!("=== Neo N3 Rust building blocks demo ===");

    // --- Storage layer ---
    let store = MemoryStore::with_columns(&[Headers::ID, Blocks::ID]);
    let header = HeaderRecord {
        hash: Hash256::new([0x11; 32]),
        height: 0,
        raw: Bytes::from(vec![0x01, 0x02, 0x03]),
    };

    let block = BlockRecord {
        hash: header.hash,
        raw: Bytes::from(vec![0xAA, 0xBB]),
    };

    if config.store {
        store.put_encoded(Headers::ID, &header.key(), &header)?;
        store.put_encoded(Blocks::ID, &block.key(), &block)?;
        println!("stored header {:?}", header.hash);
    } else {
        println!("skipping store step");
    }

    // --- Crypto layer ---
    if config.crypto {
        let private = PrivateKey::from_slice(&[0x10; 32]).expect("valid private key");
        let message = b"neo-rs";
        let signature = private.secp256r1_sign(message).expect("signing succeeds");
        let secret = SecretKey::from_slice(private.as_be_bytes()).expect("secret key");
        let signing = SigningKey::from(secret);
        let encoded = signing.verifying_key().to_encoded_point(true);
        let public = PublicKey::from_sec1_bytes(encoded.as_bytes()).expect("public key");
        public
            .secp256r1_verify(message, &signature)
            .expect("signature verified");
        println!("signature: {:02X?}", signature.as_ref());
    } else {
        println!("skipping crypto step");
    }

    // --- P2P handshake simulation ---
    if config.handshake {
        let local_endpoint = Endpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 20333);
        let remote_endpoint = Endpoint::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 20334);
        let local_version = build_version_payload(
            config.network_magic,
            0x03,
            1,
            remote_endpoint.clone(),
            local_endpoint.clone(),
            0,
        );
        let remote_version = build_version_payload(
            config.network_magic,
            0x03,
            1,
            local_endpoint.clone(),
            remote_endpoint.clone(),
            0,
        );

        let mut peer = Peer::outbound(remote_endpoint.clone(), local_version.clone());
        let mut codec = NeoMessageCodec::new();
        let mut buffer = BytesMut::new();

        for outbound in peer.bootstrap() {
            codec.encode(outbound, &mut buffer)?;
        }

        if let PeerEvent::Messages(messages) = peer.on_message(Message::Version(remote_version))? {
            for message in messages {
                codec.encode(message, &mut buffer)?;
            }
        }

        match peer.on_message(Message::Verack)? {
            PeerEvent::HandshakeCompleted | PeerEvent::None => {
                println!("handshake complete: {}", peer.is_ready());
            }
            PeerEvent::Messages(messages) => {
                println!(
                    "handshake complete with {} follow-up messages",
                    messages.len()
                );
            }
        }

        let inventory = InventoryPayload::new(vec![InventoryItem {
            kind: InventoryKind::Block,
            hash: block.hash,
        }]);
        codec.encode(Message::GetAddr, &mut buffer)?;
        codec.encode(Message::Inventory(inventory.clone()), &mut buffer)?;
        codec.encode(
            Message::Block(PayloadWithData::new(block.hash, block.raw.clone())),
            &mut buffer,
        )?;
        let addr_payload = AddressPayload::new(vec![AddressEntry::new(
            1_700_000_000,
            NetworkAddress::new(1, remote_endpoint.clone()),
        )]);
        codec.encode(Message::Address(addr_payload.clone()), &mut buffer)?;

        println!("decoding {} bytes of wire data", buffer.len());
        let mut decode_buf = buffer.clone();
        let mut decoded = Vec::new();
        while let Some(msg) = codec.decode(&mut decode_buf)? {
            decoded.push(msg.command());
        }
        println!("decoded commands: {:?}", decoded);
    } else {
        println!("skipping handshake step");
    }

    // --- Codec derived structs ---
    let demo_header = DemoHeader {
        hash: header.hash,
        payload: Bytes::from(vec![0xDE, 0xAD, 0xBE, 0xEF]),
    };
    let mut encoded = Vec::new();
    demo_header.neo_encode(&mut encoded);
    let mut reader = SliceReader::new(encoded.as_slice());
    let decoded_header = DemoHeader::neo_decode(&mut reader)?;
    assert_eq!(demo_header, decoded_header);
    println!("codec derive roundtrip ok ({} bytes)", encoded.len());

    println!("demo completed successfully");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn smoke_test() {
        run_demo(DemoConfig::default()).expect("demo run should succeed");
    }
}
