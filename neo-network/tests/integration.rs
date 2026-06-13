//! Integration smoke test for the reth-style P2P host services.

use std::net::SocketAddr;
use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_network::{
    LocalNodeService, NetworkCommand, SyncTask, SyncTaskKind, TaskId, TaskManagerService,
};
use neo_runtime::NetworkService;

#[tokio::test]
async fn local_node_handle_constructs_and_shuts_down() {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::new(settings);
    let task = tokio::spawn(service.run());
    handle.shutdown().await.expect("shutdown");
    drop(handle);
    task.await.expect("service task");
}

#[tokio::test]
async fn local_node_service_trait_object_works() {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, _handle) = LocalNodeService::new(settings);
    let service: Arc<dyn NetworkService> = Arc::new(service);
    assert_eq!(service.peer_count().await, 0);
    let mut rx = service.subscribe_events();
    assert!(rx.try_recv().is_err());
}

#[tokio::test]
async fn task_manager_handle_lifecycle() {
    let (service, handle) = TaskManagerService::new();
    let task = tokio::spawn(service.run());

    let task_id = handle
        .add_task(SyncTask::FetchBlock {
            hash: Default::default(),
            kind: SyncTaskKind::Block,
        })
        .await
        .expect("add_task");
    assert_ne!(task_id, TaskId::default());

    let active = handle.active_tasks().await.expect("active_tasks");
    assert_eq!(active.len(), 1);
    assert_eq!(active[0], task_id);

    handle
        .complete_task(task_id, Default::default())
        .await
        .expect("complete_task");
    let active = handle.active_tasks().await.expect("active_tasks");
    assert_eq!(active.len(), 0);

    handle.shutdown().await.expect("shutdown");
    task.await.expect("service task");
}

#[tokio::test]
async fn local_node_command_loop_dispatches_start() {
    let settings = Arc::new(ProtocolSettings::default());
    let (mut service, _handle) = LocalNodeService::new(settings);
    let start_addr: SocketAddr = "127.0.0.1:0".parse().unwrap();

    let (reply_tx, reply_rx) = tokio::sync::oneshot::channel();
    let start_cmd = NetworkCommand::Start {
        bind_addr: start_addr,
        reply: reply_tx,
    };
    service.dispatch(start_cmd).await;
    let result = reply_rx.await.expect("reply");
    let bound = result.expect("start should succeed");
    // Binding to port 0 resolves to a real kernel-assigned port.
    assert_ne!(bound.port(), 0);
    assert_eq!(bound.ip(), start_addr.ip());
    assert_eq!(service.peer_count().await, 0);
}

/// Await the `PeerConnected` event for any peer on `events`, with a
/// timeout, and return its `(peer_id, address)` payload.
async fn next_peer_connected(
    events: &mut tokio::sync::broadcast::Receiver<neo_network::NetworkEvent>,
) -> (String, Option<SocketAddr>) {
    loop {
        let event = tokio::time::timeout(std::time::Duration::from_secs(10), events.recv())
            .await
            .expect("timed out waiting for PeerConnected")
            .expect("event channel open");
        if let neo_network::NetworkEvent::PeerConnected { peer_id, address } = event {
            return (peer_id, address);
        }
    }
}

#[tokio::test]
async fn accept_loop_reports_inbound_peer_with_remote_address() {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::new(settings);
    let task = tokio::spawn(service.run());

    // Subscribe before the inbound connection so the event cannot be
    // missed.
    let mut events = handle.subscribe();

    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let listen_port = handle.local_node_info().port();
    assert_ne!(listen_port, 0, "start records the resolved listen port");

    // Dial the node's real accept loop from a plain TCP client.
    let stream = tokio::net::TcpStream::connect(("127.0.0.1", listen_port))
        .await
        .expect("dial local listener");
    let client_addr = stream.local_addr().expect("client local addr");
    // C# reports a peer's advertised LISTENER port and encodes an unknown
    // listener as port 0. This client never sends a version payload, so the
    // published endpoint stays (client_ip, 0) — never the ephemeral source
    // port. (The post-handshake upgrade path is covered in handshake.rs.)
    let expected = std::net::SocketAddr::new(client_addr.ip(), 0);

    let (peer_id, address) = next_peer_connected(&mut events).await;
    assert_eq!(
        address,
        Some(expected),
        "accept loop publishes (remote_ip, 0) until the peer advertises a listener port"
    );

    // The handle-side fold serves the inbound peer with its address —
    // the `getpeers` `connected` view.
    let info = handle.local_node_info();
    assert_eq!(info.connected_peers_count(), 1);
    assert_eq!(info.connected_peers()[0].peer_id, peer_id);
    assert_eq!(info.connected_peers()[0].address, Some(expected));

    drop(stream);
    handle.shutdown().await.expect("shutdown");
    drop(handle);
    task.await.expect("service task");
}

#[tokio::test]
async fn connect_peer_reports_outbound_peer_with_dialed_address() {
    // Remote end: a plain TCP listener standing in for another node.
    let remote_listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind remote listener");
    let remote_addr = remote_listener.local_addr().expect("remote addr");
    let accept_task = tokio::spawn(async move { remote_listener.accept().await });

    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::new(settings);
    let task = tokio::spawn(service.run());
    let mut events = handle.subscribe();

    handle
        .start("127.0.0.1:0".parse().unwrap())
        .await
        .expect("start");
    let peer_id = handle.connect_peer(remote_addr).await.expect("dial");

    let (event_peer_id, address) = next_peer_connected(&mut events).await;
    assert_eq!(event_peer_id, peer_id.to_string());
    assert_eq!(
        address,
        Some(remote_addr),
        "dial path publishes the dialed endpoint (the peer's listener)"
    );

    let info = handle.local_node_info();
    assert_eq!(info.connected_peers_count(), 1);
    assert_eq!(info.connected_peers()[0].peer_id, peer_id.to_string());
    assert_eq!(info.connected_peers()[0].address, Some(remote_addr));

    accept_task.await.expect("accept task").expect("accept");
    handle.shutdown().await.expect("shutdown");
    drop(handle);
    task.await.expect("service task");
}

#[tokio::test]
async fn network_handle_drop_closes_command_loop() {
    let settings = Arc::new(ProtocolSettings::default());
    let (service, handle) = LocalNodeService::new(settings);
    let task = tokio::spawn(service.run());
    drop(handle);
    let result = tokio::time::timeout(std::time::Duration::from_secs(5), task).await;
    assert!(result.is_ok(), "service should exit when handle is dropped");
}
