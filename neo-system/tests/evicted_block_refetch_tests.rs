//! A block the node gave up to stay inside its block cache budget is fetched
//! again and persisted.
//!
//! The node under test is a whole `NeoSystem`. Its only peer is the test
//! itself at the other end of a loopback connection, so every request and
//! every block passes through the real remote node, task manager and
//! blockchain actors.

use neo_ledger::{BlockCacheReservation, execution_block_cache_cost};
use neo_primitives::{UInt160, UInt256};
use neo_protocol::execution::Block;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::Witness;
use neo_system::neo_system::NeoSystem;
use neo_system::network::p2p::capabilities::NodeCapability;
use neo_system::network::p2p::connection::ReceiveStep;
use neo_system::network::p2p::payloads::VersionPayload;
use neo_system::network::p2p::{
    ChannelsConfig, FrameConfig, NetworkMessage, PeerCommand, PeerConnection, ProtocolMessage,
};
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{Duration, Instant, timeout};

/// Longest wait for anything the node is expected to do. The slowest step is
/// the task manager asking again for a block that is not on its way, which
/// it does from a five second timer once ten seconds have passed without a
/// persisted block.
const WAIT: Duration = Duration::from_secs(30);

/// How long the peer waits for the node before it looks at the test again.
const READ_STEP: Duration = Duration::from_millis(20);

/// Requests that reach the peer later than this after the node stalled were
/// sent because of the stall, not left over from the first delivery.
const SETTLE: Duration = Duration::from_secs(1);

const CHAIN_LENGTH: u32 = 6;

/// Blocks `1..=CHAIN_LENGTH` without transactions, each naming its
/// predecessor, that a node with default settings persists. Block 2 carries
/// a padded witness, which leaves its hash alone and makes it cost more than
/// three and less than four of the others, so the bytes it holds in the
/// budget cannot be mistaken for theirs.
fn chain() -> Vec<Arc<Block>> {
    let mut previous_hash = UInt256::zero();
    (1..=CHAIN_LENGTH)
        .map(|index| {
            let mut block = Block::default();
            block.header.index = index;
            block.header.previous_hash = previous_hash;
            block.header.merkle_root = UInt256::zero();
            block.header.next_consensus = UInt160::zero();
            block.header.timestamp = u64::from(index);
            block.header.witnesses = vec![if index == 2 {
                Witness::new_with_scripts(vec![0; 400], Vec::new())
            } else {
                Witness::new()
            }];
            previous_hash = block.hash();
            Arc::new(block)
        })
        .collect()
}

fn cost(block: &Block) -> usize {
    execution_block_cache_cost(block)
}

/// A node that knows the headers of `chain`, as header sync leaves it, so
/// that the blocks are verified against them when they arrive.
fn node_with_headers(chain: &[Arc<Block>]) -> Arc<NeoSystem> {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
    let header_cache = system.context().header_cache();
    for block in chain {
        assert!(header_cache.add(block.to_payload_block().header));
    }
    system
}

/// Holds all of the node's block cache budget except the room for block 2
/// of `chain`, the way blocks cached elsewhere would.
fn leave_room_for_block_two(system: &NeoSystem, chain: &[Arc<Block>]) -> BlockCacheReservation {
    let ledger = system.ledger_context();
    ledger.evict_block_bodies(usize::MAX);
    let budget = ledger.block_cache_budget();
    budget
        .try_reserve(budget.available() - cost(&chain[1]))
        .expect("ballast fits the budget")
}

/// Budget bytes held by blocks the blockchain actor has cached: whatever is
/// retained beyond `ballast` and the persisted bodies in the ledger context.
fn actor_held_bytes(system: &NeoSystem, ballast: &BlockCacheReservation) -> Option<usize> {
    let ledger = system.ledger_context();
    ledger
        .block_cache_budget()
        .retained()
        .checked_sub(ballast.bytes() + ledger.cached_block_bytes())
}

/// A request for blocks by index, as the node sent it.
struct Request {
    start: u32,
    count: i16,
    received: Instant,
}

impl Request {
    fn covers(&self, index: u32) -> bool {
        if index < self.start {
            return false;
        }
        // A negative count asks for as many blocks as the peer will send.
        match u32::try_from(self.count) {
            Ok(count) => index - self.start < count,
            Err(_) => true,
        }
    }
}

/// The remote end of the node's only connection. It has every block of the
/// chain and sends one only when the test tells it to, or, once `answers` is
/// set, whenever the node asks.
struct Peer {
    connection: PeerConnection,
    chain: Vec<Arc<Block>>,
    requests: Vec<Request>,
    answers: bool,
}

impl Peer {
    /// Has `system` dial the test and completes the handshake as a full node
    /// at the height of `chain`.
    async fn connect(system: &NeoSystem, chain: &[Arc<Block>]) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("loopback listener");
        let endpoint = listener.local_addr().expect("listener address");
        let config = ChannelsConfig {
            enable_compression: false,
            min_desired_connections: 0,
            ..Default::default()
        };
        system.start_node(config.clone()).expect("start node");
        system
            .local_node_actor()
            .tell(PeerCommand::Connect {
                endpoint,
                is_trusted: true,
            })
            .expect("dial the peer");
        let (stream, address) = timeout(WAIT, listener.accept())
            .await
            .expect("the node dials the peer")
            .expect("accept the node");

        let mut frame_config = FrameConfig::from(&config);
        frame_config.read_step_timeout = READ_STEP;
        let mut peer = Self {
            connection: PeerConnection::new(stream, address, true, frame_config),
            chain: chain.to_vec(),
            requests: Vec::new(),
            answers: false,
        };

        let height = u32::try_from(chain.len()).expect("chain length");
        assert!(matches!(
            peer.handshake_message().await,
            ProtocolMessage::Version(_)
        ));
        peer.send(ProtocolMessage::Version(VersionPayload::create(
            system.settings().network,
            42,
            "/peer".to_string(),
            vec![NodeCapability::FullNode {
                start_height: height,
            }],
        )))
        .await;
        assert!(matches!(
            peer.handshake_message().await,
            ProtocolMessage::Verack
        ));
        peer.send(ProtocolMessage::Verack).await;
        peer
    }

    async fn handshake_message(&mut self) -> ProtocolMessage {
        timeout(WAIT, self.connection.receive_message(false))
            .await
            .expect("the node continues the handshake")
            .expect("handshake message")
            .payload
    }

    async fn send(&mut self, payload: ProtocolMessage) {
        self.connection
            .send_message(&NetworkMessage::new(payload))
            .await
            .expect("send to the node");
        self.connection.flush().await.expect("flush to the node");
    }

    async fn send_blocks(&mut self, indexes: impl IntoIterator<Item = u32>) {
        for index in indexes {
            let block = self.chain[index as usize - 1].to_payload_block();
            self.send(ProtocolMessage::Block(block)).await;
        }
    }

    /// Takes at most one message from the node. Requests for blocks by index
    /// are recorded, and answered if the peer answers; anything else is
    /// ignored.
    async fn read(&mut self) {
        let step = self
            .connection
            .receive_message_step(true)
            .await
            .expect("connection to the node");
        let ReceiveStep::Message(message) = step else {
            return;
        };
        let ProtocolMessage::GetBlockByIndex(payload) = message.payload else {
            return;
        };
        let request = Request {
            start: payload.index_start,
            count: payload.count,
            received: Instant::now(),
        };
        if self.answers {
            let height = u32::try_from(self.chain.len()).expect("chain length");
            let wanted: Vec<u32> = (request.start.max(1)..=height)
                .take_while(|index| request.covers(*index))
                .collect();
            self.send_blocks(wanted).await;
        }
        self.requests.push(request);
    }

    /// Keeps reading from the node until `done` holds. Returns whether it
    /// did within [`WAIT`].
    async fn serve_until(&mut self, mut done: impl FnMut(&Self) -> bool) -> bool {
        let deadline = Instant::now() + WAIT;
        loop {
            if done(self) {
                return true;
            }
            if Instant::now() >= deadline {
                return false;
            }
            self.read().await;
        }
    }

    fn was_asked_for(&self, index: u32) -> bool {
        self.requests.iter().any(|request| request.covers(index))
    }
}

/// Delivers the chain as `3`, then `2, 4, 5, 6, 1`, to a node with room for
/// block 2 only, and waits until it has persisted what it could.
///
/// Block 3 arrives first and is cached. Block 2 is needed sooner and fits
/// only in the room block 3 and the free bytes make up together, so block 3
/// is given up for it. Blocks 4 to 6 may displace nothing and are refused.
/// Block 1 lets the node persist up to block 2, and there it stops: it has
/// been sent every block of the chain once and holds none of blocks 3 to 6.
async fn deliver_until_the_node_stalls(
    system: &NeoSystem,
    peer: &mut Peer,
    ballast: &BlockCacheReservation,
) {
    assert!(
        peer.serve_until(|peer| (1..=CHAIN_LENGTH).all(|index| peer.was_asked_for(index)))
            .await,
        "the node asks for the whole chain"
    );

    let (second, third) = (cost(&peer.chain[1]), cost(&peer.chain[2]));
    assert!(3 * third < second && second < 4 * third);

    peer.send_blocks([3]).await;
    assert!(
        peer.serve_until(|_| actor_held_bytes(system, ballast) == Some(third))
            .await,
        "block 3 is cached"
    );

    // Once block 2 is persisted the actor may keep its entry as the tip, or
    // not. No number of the other blocks costs what block 2 does, so any
    // other amount would be a block above the tip.
    peer.send_blocks([2, 4, 5, 6, 1]).await;
    assert!(
        peer.serve_until(|_| {
            system.current_block_index() == 2
                && [Some(0), Some(second)].contains(&actor_held_bytes(system, ballast))
        })
        .await,
        "blocks 1 and 2 are persisted and no block above them is held"
    );
}

#[tokio::test]
async fn block_given_up_for_memory_is_requested_again_and_persisted() {
    let chain = chain();
    let system = node_with_headers(&chain);
    let ballast = leave_room_for_block_two(&system, &chain);
    let mut peer = Peer::connect(&system, &chain).await;

    deliver_until_the_node_stalls(&system, &mut peer, &ballast).await;
    let stalled = Instant::now();

    // The node asked for block 3 once more as soon as it had arrived, while
    // it still held it. The peer leaves that request open, like every other
    // one, so nothing is on its way to the node. The node has to notice on
    // its own that it is not getting anywhere and ask again.
    let asked_again = peer
        .serve_until(|peer| {
            peer.requests
                .iter()
                .any(|request| request.covers(3) && request.received >= stalled + SETTLE)
        })
        .await;
    assert!(asked_again, "the node asks again for the block it gave up");
    assert_eq!(system.current_block_index(), 2);

    // The same block over the same connection is taken this time.
    peer.send_blocks([3]).await;
    assert!(
        peer.serve_until(|_| system.current_block_index() == 3)
            .await,
        "the block is persisted when it is sent again"
    );

    // Blocks 4 to 6 were refused outright. With a peer that answers, the
    // node gets them too and reaches the tip.
    peer.answers = true;
    peer.send_blocks(4..=CHAIN_LENGTH).await;
    assert!(
        peer.serve_until(|_| system.current_block_index() == CHAIN_LENGTH)
            .await,
        "sync resumes up to the peer's height"
    );

    drop(ballast);
    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn blocks_given_up_for_memory_are_persisted_when_the_peer_sends_them_again() {
    let chain = chain();
    let system = node_with_headers(&chain);
    let ballast = leave_room_for_block_two(&system, &chain);
    let mut peer = Peer::connect(&system, &chain).await;

    deliver_until_the_node_stalls(&system, &mut peer, &ballast).await;

    // Every one of these blocks has been through this connection before,
    // and the node takes each of them once its turn has come. From here on
    // the peer answers whatever it is asked.
    peer.answers = true;
    peer.send_blocks(3..=CHAIN_LENGTH).await;
    assert!(
        peer.serve_until(|_| system.current_block_index() == CHAIN_LENGTH)
            .await,
        "sync resumes up to the peer's height"
    );

    drop(ballast);
    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn same_delivery_needs_no_second_request_when_the_budget_has_room() {
    let chain = chain();
    let system = node_with_headers(&chain);
    let mut peer = Peer::connect(&system, &chain).await;

    assert!(
        peer.serve_until(|peer| (1..=CHAIN_LENGTH).all(|index| peer.was_asked_for(index)))
            .await,
        "the node asks for the whole chain"
    );

    // The delivery that stalls a node short of memory at block 2. Here every
    // block is kept until its turn, and the peer never sends one twice.
    peer.send_blocks([3, 2, 4, 5, 6, 1]).await;
    assert!(
        peer.serve_until(|_| system.current_block_index() == CHAIN_LENGTH)
            .await,
        "every block is persisted from the first delivery"
    );

    system.shutdown().await.expect("shutdown");
}
