use super::helpers::PersistCompletedHarness;
use crate::messages::{ConsensusPayload, PrepareRequestMessage};
use crate::{ConsensusEvent, ConsensusMessageType, ConsensusService};
use std::sync::Arc;
use tokio::net::UdpSocket;
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

async fn run_node(
    mut service: ConsensusService,
    mut events: mpsc::Receiver<ConsensusEvent>,
    socket: Arc<UdpSocket>,
    peers: Vec<std::net::SocketAddr>,
    committed: mpsc::Sender<u32>,
) {
    let mut buf = vec![0u8; 65_535];
    loop {
        tokio::select! {
            event = events.recv() => {
                match event {
                    Some(ConsensusEvent::BroadcastMessage(payload)) => {                        let bytes = encode(&payload);
                        for peer in &peers {
                            let _ = socket.send_to(&bytes, *peer).await;
                        }
                    }
                    Some(ConsensusEvent::RequestTransactions { .. }) => {
                        let _ = service.on_transactions_received(Vec::new());
                    }
                    Some(ConsensusEvent::BlockCommitted { block_index, .. }) => {
                        let _ = committed.send(block_index).await;
                        return;
                    }
                    Some(_) => {}
                    None => return,
                }
            }
            received = socket.recv_from(&mut buf) => {
                let Ok((len, _from)) = received else { continue };
                let Some(payload) = decode(&buf[..len]) else { continue };                let prepare = if payload.message_type == ConsensusMessageType::PrepareRequest {
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
async fn four_validators_commit_over_real_udp_transport() {
    let network = 0x4E454F;
    let mut harness = PersistCompletedHarness::new(network, 4);
    harness
        .persist_completed_all(0, neo_primitives::UInt256::from([1u8; 32]), 1_700_000_000_000)
        .expect("start consensus");
    harness.fire_initial_timers().expect("primary proposal");
    let (mut services, mut receivers) = harness.into_parts();
    let mut sockets = Vec::new();
    let mut addresses = Vec::new();
    for _ in 0..4 {
        let socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.expect("bind udp"));
        addresses.push(socket.local_addr().expect("local addr"));
        sockets.push(socket);
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
            Arc::clone(&sockets[index]),
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
