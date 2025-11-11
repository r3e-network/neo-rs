use std::{
    collections::HashSet,
    fs,
    net::SocketAddr,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use clap::Parser;
use futures::{SinkExt, StreamExt};
use neo_base::hash::Hash256;
use neo_p2p::{
    build_version_payload,
    message::{
        AddressPayload, Endpoint, GetBlockByIndexPayload, InventoryItem, InventoryPayload, Message,
        PayloadWithData, PingPayload,
    },
    Capability, NeoMessageCodec, Peer, PeerEvent,
};
use neo_store::{BlockRecord, Blocks, Column, MemoryStore, StoreExt};
use rand::random;
use tokio::net::TcpStream;
use tokio::time::timeout;
use tokio_util::codec::Framed;

#[derive(Parser, Debug)]
struct Options {
    /// Remote peer address (ip:port).
    #[arg(default_value = "127.0.0.1:20333")]
    target: SocketAddr,

    /// Network magic (mainnet=860833102, testnet=877933390).
    #[arg(long, default_value_t = 860_833_102u32)]
    network: u32,

    /// Protocol version to announce.
    #[arg(long, default_value_t = 0x03u32)]
    protocol: u32,

    /// User agent to advertise in the version payload.
    #[arg(long, default_value = "/neo-rs-handshake")]
    user_agent: String,

    /// Seconds to wait for the next inbound message.
    #[arg(long, default_value_t = 15)]
    timeout_secs: u64,

    /// Request block headers after the handshake.
    #[arg(long)]
    request_headers: bool,

    /// Starting height when requesting headers.
    #[arg(long, default_value_t = 0)]
    start_index: u32,

    /// Whether to issue `getdata` for every inventory announcement.
    #[arg(long)]
    request_data: bool,

    /// Store retrieved block payloads in an in-memory neo-store column.
    #[arg(long)]
    store_blocks: bool,

    /// Track retrieved transactions for optional dumping.
    #[arg(long)]
    store_txs: bool,

    /// Directory to dump stored payloads as raw bytes.
    #[arg(long)]
    dump_dir: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let options = Options::parse();
    let endpoint = Endpoint::new(options.target.ip(), options.target.port());
    let local_caps = vec![Capability::tcp_server(0), Capability::full_node(0)];
    let local_version = build_version_payload(
        options.network,
        options.protocol,
        options.user_agent.clone(),
        local_caps,
    );

    let mut peer = Peer::outbound(endpoint, local_version);
    let mut pending_requests: HashSet<Hash256> = HashSet::new();
    let mut sink = PayloadSink::new(
        options.store_blocks,
        options.store_txs,
        options.dump_dir.clone(),
    );
    let stream = TcpStream::connect(options.target)
        .await
        .context("failed to open TCP connection")?;
    let mut framed = Framed::new(
        stream,
        NeoMessageCodec::new().with_network_magic(options.network),
    );

    println!("connecting to {}", options.target);
    for message in peer.bootstrap() {
        print_send(&message);
        framed.send(message).await?;
    }

    let mut handshake_logged = false;
    loop {
        let next = timeout(Duration::from_secs(options.timeout_secs), framed.next()).await;
        let maybe_message = match next {
            Ok(Some(Ok(message))) => Some(message),
            Ok(Some(Err(err))) => return Err(err.into()),
            Ok(None) => {
                println!("remote closed the connection");
                break;
            }
            Err(_) => {
                println!("timed out waiting for the next message");
                break;
            }
        };

        let Some(message) = maybe_message else {
            break;
        };

        print_recv(&message);
        if !peer.is_ready() {
            match peer.on_message(message)? {
                PeerEvent::Messages(responses) => {
                    for response in responses {
                        print_send(&response);
                        framed.send(response).await?;
                    }
                }
                PeerEvent::HandshakeCompleted => {
                    if !handshake_logged {
                        on_handshake_ready(&peer, &mut framed, &options).await?;
                        handshake_logged = true;
                    }
                }
                PeerEvent::None => {}
            }
            continue;
        }

        handle_post_handshake(
            &mut framed,
            &mut pending_requests,
            &mut sink,
            &options,
            message,
        )
        .await?;
    }

    sink.finish();
    Ok(())
}

async fn on_handshake_ready(
    peer: &Peer,
    framed: &mut Framed<TcpStream, NeoMessageCodec>,
    options: &Options,
) -> Result<()> {
    println!(
        "handshake complete (compression: {})",
        peer.compression_allowed()
    );
    framed
        .codec_mut()
        .set_compression_allowed(peer.compression_allowed());
    let getaddr = Message::GetAddr;
    print_send(&getaddr);
    framed.send(getaddr).await?;
    println!("requested peer addresses");

    let ping = build_ping_message();
    print_send(&ping);
    framed.send(ping).await?;

    if options.request_headers {
        let payload = GetBlockByIndexPayload {
            start_index: options.start_index,
            count: -1,
        };
        let headers = Message::GetHeaders(payload);
        print_send(&headers);
        framed.send(headers).await?;
        println!("requested headers starting at {}", options.start_index);
    }

    Ok(())
}

fn print_send(message: &Message) {
    println!("-> {}", message.command().as_str());
}

fn print_recv(message: &Message) {
    println!("<- {}", message.command().as_str());
    if let Message::Address(payload) = message {
        describe_addresses(payload);
    }
}

fn describe_addresses(payload: &AddressPayload) {
    println!("   received {} addresses", payload.entries.len());
    for entry in payload.entries.iter().take(5) {
        let endpoint = &entry.address.endpoint;
        println!(
            "   - {}:{} (services: {})",
            endpoint.address, endpoint.port, entry.address.services
        );
    }
    if payload.entries.len() > 5 {
        println!("   ...");
    }
}

fn print_inventory(payload: &InventoryPayload) {
    println!("   inventory items: {}", payload.items.len());
    for item in payload.items.iter().take(5) {
        println!("   - {:?} {}", item.kind, item.hash);
    }
    if payload.items.len() > 5 {
        println!("   ...");
    }
}

async fn handle_post_handshake(
    framed: &mut Framed<TcpStream, NeoMessageCodec>,
    pending: &mut HashSet<Hash256>,
    sink: &mut PayloadSink,
    options: &Options,
    message: Message,
) -> Result<()> {
    match message {
        Message::Ping(payload) => {
            println!("   responding with pong");
            let response = Message::Pong(payload.clone());
            print_send(&response);
            framed.send(response).await?;
        }
        Message::Pong(_) => println!("   received pong"),
        Message::Headers(payload) => {
            println!("   received {} headers", payload.headers.len());
        }
        Message::Inventory(payload) => {
            print_inventory(&payload);
            if options.request_data {
                let new_items: Vec<InventoryItem> = payload
                    .items
                    .iter()
                    .filter(|item| pending.insert(item.hash))
                    .cloned()
                    .collect();
                if !new_items.is_empty() {
                    let request = Message::GetData(InventoryPayload::new(new_items));
                    print_send(&request);
                    framed.send(request).await?;
                }
            }
        }
        Message::Block(payload) => {
            pending.remove(&payload.hash);
            println!(
                "   received block {} ({} bytes)",
                payload.hash,
                payload.data.len()
            );
            sink.record_block(&payload);
        }
        Message::Transaction(payload) => {
            pending.remove(&payload.hash);
            println!(
                "   received transaction {} ({} bytes)",
                payload.hash,
                payload.data.len()
            );
            sink.record_transaction(&payload);
        }
        Message::Address(_) => {} // already logged via print_recv
        _ => {}
    }
    Ok(())
}

fn build_ping_message() -> Message {
    Message::Ping(PingPayload {
        last_block_index: 0,
        timestamp: current_timestamp(),
        nonce: random(),
    })
}

fn current_timestamp() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as u32
}

struct PayloadSink {
    block_store: Option<MemoryStore>,
    dump_dir: Option<PathBuf>,
    stored_blocks: usize,
    stored_txs: usize,
    dump_blocks: Vec<PayloadWithData>,
    dump_txs: Vec<PayloadWithData>,
    store_txs: bool,
}

impl PayloadSink {
    fn new(store_blocks: bool, store_txs: bool, dump_dir: Option<PathBuf>) -> Self {
        let block_store = store_blocks.then(|| MemoryStore::with_columns(&[Blocks::ID]));
        Self {
            block_store,
            dump_dir,
            stored_blocks: 0,
            stored_txs: 0,
            dump_blocks: Vec::new(),
            dump_txs: Vec::new(),
            store_txs,
        }
    }

    fn record_block(&mut self, payload: &PayloadWithData) {
        if let Some(store) = &self.block_store {
            let record = BlockRecord {
                hash: payload.hash,
                raw: payload.data.clone(),
            };
            if let Err(err) = store.put_encoded(Blocks::ID, &record.key(), &record) {
                eprintln!("failed to store block {}: {}", payload.hash, err);
            } else {
                self.stored_blocks += 1;
            }
        }
        if self.dump_dir.is_some() {
            self.dump_blocks.push(payload.clone());
        }
    }

    fn record_transaction(&mut self, payload: &PayloadWithData) {
        if self.store_txs || self.dump_dir.is_some() {
            self.dump_txs.push(payload.clone());
            self.stored_txs += 1;
        }
    }

    fn finish(&self) {
        if self.block_store.is_some() {
            println!("stored {} block(s) in memory", self.stored_blocks);
        }
        if self.store_txs || self.dump_dir.is_some() {
            println!("tracked {} transaction(s)", self.stored_txs);
        }
        if let Some(dir) = &self.dump_dir {
            if let Err(err) = fs::create_dir_all(dir) {
                eprintln!("failed to create dump dir {}: {}", dir.display(), err);
                return;
            }
            for payload in &self.dump_blocks {
                let path = dir.join(format!("block_{}.bin", payload.hash));
                if let Err(err) = fs::write(&path, payload.data.as_slice()) {
                    eprintln!("failed to write {}: {}", path.display(), err);
                } else {
                    println!("wrote {}", path.display());
                }
            }
            for payload in &self.dump_txs {
                let path = dir.join(format!("tx_{}.bin", payload.hash));
                if let Err(err) = fs::write(&path, payload.data.as_slice()) {
                    eprintln!("failed to write {}: {}", path.display(), err);
                } else {
                    println!("wrote {}", path.display());
                }
            }
        }
    }
}
