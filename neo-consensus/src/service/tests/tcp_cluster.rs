use super::helpers::PersistCompletedHarness;
use crate::messages::{ConsensusPayload, PrepareRequestMessage};
use crate::{ConsensusEvent, ConsensusMessageType, ConsensusService};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

fn encode(payload: &ConsensusPayload) -> Vec<u8> {
    let message = payload.to_message_bytes();
    let mut bytes = Vec::with_capacity(4 + 4 + payload.witness.len() + message.len());
    bytes.extend_from_slice(&payload.network.to_le_bytes());
    bytes.extend_from_slice(&(payload.witness.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&payload.witness);
    bytes.extend_from_slice(&message);
    bytes
}

fn decode(bytes: &[u8]) -> Option<ConsensusPayload> {
    if bytes.len() < 8 {
        return None;
    }
    let network = u32::from_le_bytes(bytes[0..4].try_into().ok()?);
    let witness_len = u32::from_le_bytes(bytes[4..8].try_into().ok()?) as usize;
    let witness_end = 8 + witness_len;
    if bytes.len() < witness_end + 7 {
        return None;
    }
    let witness = bytes[8..witness_end].to_vec();
    let message = &bytes[witness_end..];
    ConsensusPayload::from_message_bytes(network, message, witness).ok()
}

async fn read_frame(stream: &mut TcpStream) -> Option<Vec<u8>> {
    let mut length = [0u8; 4];
    stream.read_exact(&mut length).await.ok()?;
    let length = u32::from_le_bytes(length) as usize;
    if length == 0 || length > 1 << 20 {
        return None;
    }
    let mut bytes = vec![0u8; length];
    stream.read_exact(&mut bytes).await.ok()?;
    Some(bytes)
}

async fn run_node(
    mut service: ConsensusService,
    mut events: mpsc::Receiver<ConsensusEvent>,
    listener: TcpListener,
    peers: Vec<std::net::SocketAddr>,
    committed: mpsc::Sender<u32>,
) {
    let (inbound_tx, mut inbound_rx) = mpsc::channel::<Vec<u8>>(256);
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let inbound = inbound_tx.clone();
            tokio::spawn(async move {
                if let Some(bytes) = read_frame(&mut stream).await {
                    let _ = inbound.send(bytes).await;
                }
            });
        }
    });

    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Some(ConsensusEvent::RequestTransactions { .. }) => {
                        let _ = service.on_transactions_received(Vec::new());
                    }
                    Some(ConsensusEvent::BroadcastMessage(payload)) => {
                        let bytes = encode(&payload);
                        let length = (bytes.len() as u32).to_le_bytes();
                        for peer in &peers {
                            if let Ok(mut stream) = TcpStream::connect(peer).await
                                && stream.write_all(&length).await.is_ok()
                            {
                                let _ = stream.write_all(&bytes).await;
                            }
                        }
                    }
                    Some(ConsensusEvent::BlockCommitted { block_index, .. }) => {
                        let _ = committed.send(block_index).await;
                        return;
                    }
                    Some(_) => {}
                    None => return,
                }
            }
            received = inbound_rx.recv() => {
                let Some(bytes) = received else { continue };
                let Some(payload) = decode(&bytes) else { continue };
                let prepare = if payload.message_type == ConsensusMessageType::PrepareRequest {
                    PrepareRequestMessage::deserialize_body(
                        &payload.data,
                        payload.block_index,
                        payload.view_number,
                        payload.validator_index,
                    )
                    .ok()
                } else {
                    None
                };
                if service.process_message(payload).is_err() {
                    continue;
                }
                if let Some(prepare) = prepare {
                    let _ = service.on_transactions_received(prepare.transaction_hashes);
                }
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn four_validators_commit_over_real_tcp_transport() {
    let network = 0x4E454F;
    let mut harness = PersistCompletedHarness::new(network, 4);
    harness
        .persist_completed_all(0, neo_primitives::UInt256::from([1u8; 32]), 1_700_000_000_000)
        .expect("start consensus");
    harness.fire_initial_timers().expect("primary proposal");
    let (mut services, mut receivers) = harness.into_parts();

    let mut listeners = Vec::new();
    let mut addresses = Vec::new();
    for _ in 0..4 {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind tcp");
        addresses.push(listener.local_addr().expect("local addr"));
        listeners.push(listener);
    }

    let (committed_tx, mut committed_rx) = mpsc::channel(8);
    for index in 0..4 {
        let peers = addresses
            .iter()
            .enumerate()
            .filter(|(peer, _)| *peer != index)
            .map(|(_, address)| *address)
            .collect();
        tokio::spawn(run_node(
            services.remove(0),
            receivers.remove(0),
            listeners.remove(0),
            peers,
            committed_tx.clone(),
        ));
    }

    let committed = tokio::time::timeout(std::time::Duration::from_secs(10), committed_rx.recv())
        .await
        .expect("consensus timeout")
        .expect("commit event");
    assert_eq!(committed, 1);
}
