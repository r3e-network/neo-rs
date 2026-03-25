#![cfg(feature = "runtime")]

use async_trait::async_trait;
use neo_core::actors::{Actor, ActorContext, ActorRef, ActorResult, ActorSystem, Props};
use neo_core::network::p2p::capabilities::NodeCapability;
use neo_core::network::p2p::payloads::{ExtensiblePayload, VersionPayload};
use neo_core::network::p2p::{
    ChannelsConfig, LocalNode, LocalNodeCommand, PeerCommand, RelayInventory, RemoteNodeCommand,
    RemoteNodeSnapshot,
};
use neo_core::network::MessageCommand;
use neo_core::protocol_settings::ProtocolSettings;
use std::any::Any;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::sync::oneshot;
use tokio::time::{timeout, Duration};

struct CaptureActor {
    tx: mpsc::UnboundedSender<RemoteNodeCommand>,
}

#[async_trait]
impl Actor for CaptureActor {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(command) = message.downcast::<RemoteNodeCommand>() {
            let _ = self.tx.send((*command).clone());
        }
        Ok(())
    }
}

fn peer_version(settings: &ProtocolSettings) -> VersionPayload {
    VersionPayload::create(
        settings,
        42,
        "/peer".to_string(),
        vec![
            NodeCapability::FullNode { start_height: 0 },
            NodeCapability::tcp_server(20333),
        ],
    )
}

fn peer_snapshot(version: &VersionPayload) -> RemoteNodeSnapshot {
    RemoteNodeSnapshot {
        remote_address: "10.0.0.9:40000".parse().expect("remote address"),
        remote_port: 40000,
        listen_tcp_port: 20333,
        last_block_index: 0,
        version: version.version,
        services: 0,
        timestamp: 1,
    }
}

fn extensible_inventory() -> RelayInventory {
    let mut payload = ExtensiblePayload::new();
    payload.category = "dBFT".to_string();
    payload.valid_block_end = 1;
    RelayInventory::Extensible(payload)
}

async fn unconnected_count(local_actor: &ActorRef) -> usize {
    let (reply_tx, reply_rx) = oneshot::channel();
    local_actor
        .tell(LocalNodeCommand::UnconnectedCount { reply: reply_tx })
        .expect("query unconnected count");
    reply_rx.await.expect("unconnected count")
}

async fn connecting_peers(local_actor: &ActorRef) -> Vec<std::net::SocketAddr> {
    let (reply_tx, reply_rx) = oneshot::channel();
    local_actor
        .tell(PeerCommand::QueryConnectingPeers { reply: reply_tx })
        .expect("query connecting peers");
    reply_rx.await.expect("connecting peers")
}

#[tokio::test]
async fn relay_directly_announces_inventory_to_remote_nodes() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    let system = ActorSystem::new("local-node-relay").expect("actor system");
    let local_actor = system
        .actor_of(LocalNode::props(node.clone()), "local-node")
        .expect("local actor");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let probe = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx.clone() }),
            "remote-probe-relay",
        )
        .expect("probe actor");

    let version = peer_version(&settings);
    node.register_remote_node(probe, peer_snapshot(&version), version);

    local_actor
        .tell(LocalNodeCommand::RelayDirectly {
            inventory: extensible_inventory(),
            block_index: None,
        })
        .expect("send relay command");

    let command = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for relay command")
        .expect("relay command");

    assert!(matches!(
        command,
        RemoteNodeCommand::RelayInventory(RelayInventory::Extensible(_))
    ));

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn send_directly_pushes_full_inventory_to_remote_nodes() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    let system = ActorSystem::new("local-node-send").expect("actor system");
    let local_actor = system
        .actor_of(LocalNode::props(node.clone()), "local-node")
        .expect("local actor");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let probe = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx.clone() }),
            "remote-probe-send",
        )
        .expect("probe actor");

    let version = peer_version(&settings);
    node.register_remote_node(probe, peer_snapshot(&version), version);

    local_actor
        .tell(LocalNodeCommand::SendDirectly {
            inventory: extensible_inventory(),
            block_index: None,
        })
        .expect("send direct command");

    let command = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for send command")
        .expect("send command");

    assert!(matches!(
        command,
        RemoteNodeCommand::SendInventory {
            inventory: RelayInventory::Extensible(_)
        }
    ));

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn address_book_ignores_peers_without_tcp_server_capability() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    let system = ActorSystem::new("local-node-address-book").expect("actor system");

    let (tx, _rx) = mpsc::unbounded_channel();
    let probe = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx.clone() }),
            "remote-probe-address-book",
        )
        .expect("probe actor");

    let version = VersionPayload::create(
        &settings,
        99,
        "/peer".to_string(),
        vec![NodeCapability::FullNode { start_height: 0 }],
    );
    let snapshot = RemoteNodeSnapshot {
        remote_address: "10.0.0.10:40001".parse().expect("remote address"),
        remote_port: 40001,
        listen_tcp_port: 0,
        last_block_index: 0,
        version: version.version,
        services: 0,
        timestamp: 1,
    };
    node.register_remote_node(probe, snapshot, version);

    assert!(node.address_book().is_empty());

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn timer_elapsed_with_connected_peers_uses_getaddr_without_seeding() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    node.set_seed_list(vec!["127.0.0.2:20333".to_string()]);

    let system = ActorSystem::new("local-node-need-more-peers").expect("actor system");
    let local_actor = system
        .actor_of(LocalNode::props(node.clone()), "local-node")
        .expect("local actor");

    local_actor
        .tell(PeerCommand::Configure {
            config: ChannelsConfig {
                min_desired_connections: 2,
                ..Default::default()
            },
        })
        .expect("configure");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let probe = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx.clone() }),
            "remote-probe-peers",
        )
        .expect("probe actor");

    let version = peer_version(&settings);
    let (reply_tx, reply_rx) = oneshot::channel();
    local_actor
        .tell(PeerCommand::ConnectionEstablished {
            actor: probe,
            snapshot: peer_snapshot(&version),
            is_trusted: false,
            inbound: false,
            version,
            reply: reply_tx,
        })
        .expect("register connected peer");
    assert!(reply_rx.await.expect("reply"));

    local_actor
        .tell(PeerCommand::TimerElapsed)
        .expect("trigger timer");

    let command = timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for getaddr")
        .expect("command");
    match command {
        RemoteNodeCommand::Send(message) => assert_eq!(message.command(), MessageCommand::GetAddr),
        other => panic!("expected getaddr send, got {other:?}"),
    }

    let (count_tx, count_rx) = oneshot::channel();
    local_actor
        .tell(LocalNodeCommand::UnconnectedCount { reply: count_tx })
        .expect("query unconnected count");
    let count = count_rx.await.expect("count");
    assert_eq!(count, 0, "connected peers should suppress seed fallback");

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn timer_elapsed_does_not_request_more_peers_when_unconnected_pool_is_non_empty() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    let system = ActorSystem::new("local-node-existing-unconnected").expect("actor system");
    let local_actor = system
        .actor_of(LocalNode::props(node.clone()), "local-node")
        .expect("local actor");

    local_actor
        .tell(PeerCommand::Configure {
            config: ChannelsConfig {
                min_desired_connections: 2,
                ..Default::default()
            },
        })
        .expect("configure");

    let (tx, mut rx) = mpsc::unbounded_channel();
    let probe = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx.clone() }),
            "remote-probe-existing-unconnected",
        )
        .expect("probe actor");

    let version = peer_version(&settings);
    let (reply_tx, reply_rx) = oneshot::channel();
    local_actor
        .tell(PeerCommand::ConnectionEstablished {
            actor: probe,
            snapshot: peer_snapshot(&version),
            is_trusted: false,
            inbound: false,
            version,
            reply: reply_tx,
        })
        .expect("register connected peer");
    assert!(reply_rx.await.expect("reply"));

    local_actor
        .tell(LocalNodeCommand::AddUnconnectedPeers {
            endpoints: vec!["127.0.0.2:20333".parse().expect("endpoint")],
        })
        .expect("add unconnected peer");

    while rx.try_recv().is_ok() {}

    local_actor
        .tell(PeerCommand::TimerElapsed)
        .expect("trigger timer");

    let result = timeout(Duration::from_millis(300), rx.recv()).await;
    assert!(
        result.is_err(),
        "existing unconnected peers should suppress getaddr discovery"
    );

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn timer_elapsed_drops_endpoint_when_connect_attempt_is_rejected() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    let system = ActorSystem::new("local-node-drop-rejected-target").expect("actor system");
    let local_actor = system
        .actor_of(LocalNode::props(node.clone()), "local-node")
        .expect("local actor");

    local_actor
        .tell(PeerCommand::Configure {
            config: ChannelsConfig {
                min_desired_connections: 1,
                max_connections_per_address: 0,
                ..Default::default()
            },
        })
        .expect("configure");

    local_actor
        .tell(LocalNodeCommand::AddUnconnectedPeers {
            endpoints: vec!["127.0.0.2:20333".parse().expect("endpoint")],
        })
        .expect("add unconnected peer");

    local_actor
        .tell(PeerCommand::TimerElapsed)
        .expect("trigger timer");

    assert_eq!(
        unconnected_count(&local_actor).await,
        0,
        "rejected connect targets should not be requeued automatically"
    );

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn address_book_groups_same_ip_and_uses_version_timestamp() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    let system = ActorSystem::new("local-node-address-book-grouping").expect("actor system");

    let (tx_a, _rx_a) = mpsc::unbounded_channel();
    let (tx_b, _rx_b) = mpsc::unbounded_channel();
    let probe_a = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx_a.clone() }),
            "remote-probe-address-a",
        )
        .expect("probe actor a");
    let probe_b = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx_b.clone() }),
            "remote-probe-address-b",
        )
        .expect("probe actor b");

    let mut version_a = peer_version(&settings);
    version_a.timestamp = 111;
    let mut version_b = peer_version(&settings);
    version_b.timestamp = 111;

    node.register_remote_node(
        probe_a,
        RemoteNodeSnapshot {
            remote_address: "10.0.0.11:40001".parse().expect("remote a"),
            remote_port: 40001,
            listen_tcp_port: 20333,
            last_block_index: 0,
            version: version_a.version,
            services: 0,
            timestamp: 9_999,
        },
        version_a.clone(),
    );
    node.register_remote_node(
        probe_b,
        RemoteNodeSnapshot {
            remote_address: "10.0.0.11:40002".parse().expect("remote b"),
            remote_port: 40002,
            listen_tcp_port: 30333,
            last_block_index: 0,
            version: version_b.version,
            services: 0,
            timestamp: 8_888,
        },
        version_b,
    );

    let addresses = node.address_book();
    assert_eq!(addresses.len(), 1, "Neo groups addr responses by peer IP");
    assert_eq!(addresses[0].timestamp, 111);

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn handshake_failure_does_not_requeue_outbound_endpoint() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    let system = ActorSystem::new("local-node-no-handshake-requeue").expect("actor system");
    let local_actor = system
        .actor_of(LocalNode::props(node.clone()), "local-node")
        .expect("local actor");

    let endpoint = "35.238.34.240:20333".parse().expect("endpoint");
    node.track_pending(endpoint);

    local_actor
        .tell(PeerCommand::ConnectionFailed { endpoint })
        .expect("report handshake failure");

    assert_eq!(
        unconnected_count(&local_actor).await,
        0,
        "failed handshakes should not be requeued immediately"
    );

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn connect_before_config_is_deferred_until_configure() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    let system = ActorSystem::new("local-node-connect-pre-config").expect("actor system");
    let local_actor = system
        .actor_of(LocalNode::props(node.clone()), "local-node")
        .expect("local actor");

    let endpoint = "192.0.2.1:20333".parse().expect("endpoint");
    local_actor
        .tell(PeerCommand::Connect {
            endpoint,
            is_trusted: false,
        })
        .expect("connect before config");

    assert!(
        !connecting_peers(&local_actor).await.contains(&endpoint),
        "pre-config connect should be deferred until configuration arrives"
    );

    local_actor
        .tell(PeerCommand::Configure {
            config: ChannelsConfig {
                min_desired_connections: 1,
                ..Default::default()
            },
        })
        .expect("configure");

    timeout(Duration::from_millis(200), async {
        loop {
            if connecting_peers(&local_actor).await.contains(&endpoint) {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("configured local node should process the deferred connect request");

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn outbound_tcp_failure_does_not_requeue_endpoint() {
    let settings = Arc::new(ProtocolSettings::default());
    let node = Arc::new(LocalNode::new(
        settings.clone(),
        10333,
        "/agent".to_string(),
    ));
    let system = ActorSystem::new("local-node-no-tcp-failure-requeue").expect("actor system");
    let local_actor = system
        .actor_of(LocalNode::props(node.clone()), "local-node")
        .expect("local actor");

    let endpoint = "35.238.34.240:20333".parse().expect("endpoint");
    node.track_pending(endpoint);

    local_actor
        .tell(LocalNodeCommand::OutboundTcpFailed {
            endpoint,
            is_trusted: false,
            error: "timed out".to_string(),
            timed_out: true,
            permission_denied: false,
        })
        .expect("report tcp failure");

    assert_eq!(
        unconnected_count(&local_actor).await,
        0,
        "failed tcp dials should not be requeued immediately"
    );

    system.shutdown().await.expect("shutdown");
}
