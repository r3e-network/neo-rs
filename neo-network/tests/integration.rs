//! Integration smoke test for the reth-style P2P host services.

use std::sync::Arc;
use std::net::SocketAddr;

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
    assert!(result.is_ok(), "start should succeed: {result:?}");
    assert_eq!(service.peer_count().await, 0);
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
