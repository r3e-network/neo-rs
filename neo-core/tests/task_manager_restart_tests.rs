#![cfg(feature = "runtime")]

use async_trait::async_trait;
use neo_core::actors::{Actor, ActorContext, ActorResult, ActorSystem, Props};
use neo_core::neo_system::NeoSystem;
use neo_core::network::p2p::messages::ProtocolMessage;
use neo_core::network::p2p::payloads::{InvPayload, InventoryType, VersionPayload};
use neo_core::network::p2p::{
    MessageCommand, NetworkMessage, RemoteNodeCommand, TaskManagerActor, TaskManagerCommand,
};
use neo_core::UInt256;
use std::any::Any;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::sync::mpsc;
use tokio::time::{timeout, Duration};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};
use tracing_subscriber::prelude::*;

struct CaptureActor {
    tx: mpsc::UnboundedSender<MessageCommand>,
}

#[async_trait]
impl Actor for CaptureActor {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(command) = message.downcast::<RemoteNodeCommand>() {
            if let RemoteNodeCommand::Send(message) = *command {
                let _ = self.tx.send(message.command());
            }
        }
        Ok(())
    }
}

struct MessageCaptureActor {
    tx: mpsc::UnboundedSender<NetworkMessage>,
}

#[async_trait]
impl Actor for MessageCaptureActor {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(command) = message.downcast::<RemoteNodeCommand>() {
            if let RemoteNodeCommand::Send(message) = *command {
                let _ = self.tx.send(message);
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
struct WarnCountLayer {
    count: Arc<AtomicUsize>,
}

impl<S> Layer<S> for WarnCountLayer
where
    S: Subscriber,
{
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() == Level::WARN && event.metadata().target() == "neo" {
            self.count.fetch_add(1, Ordering::SeqCst);
        }
    }
}

#[tokio::test]
async fn restart_tasks_from_non_session_sender_broadcasts_getdata() {
    let system = ActorSystem::new("task-manager-restart-runtime").expect("actor system");
    let task_manager = system
        .actor_of(Props::new(TaskManagerActor::default), "task-manager")
        .expect("task manager actor");

    let (tx_a, mut rx_a) = mpsc::unbounded_channel();
    let (tx_b, mut rx_b) = mpsc::unbounded_channel();
    let actor_a = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx_a.clone() }),
            "peer-a",
        )
        .expect("peer a");
    let actor_b = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx_b.clone() }),
            "peer-b",
        )
        .expect("peer b");
    let unknown_sender = system
        .actor_of(
            Props::new(move || CaptureActor {
                tx: mpsc::unbounded_channel().0,
            }),
            "unknown-sender",
        )
        .expect("unknown sender");

    let version = VersionPayload::default();
    task_manager
        .tell_from(
            TaskManagerCommand::Register {
                version: version.clone(),
            },
            Some(actor_a.clone()),
        )
        .expect("register a");
    task_manager
        .tell_from(
            TaskManagerCommand::Register { version },
            Some(actor_b.clone()),
        )
        .expect("register b");

    let hash = UInt256::from([9u8; 32]);
    task_manager
        .tell_from(
            TaskManagerCommand::RestartTasks {
                payload: InvPayload::create(InventoryType::Transaction, &[hash]),
            },
            Some(unknown_sender),
        )
        .expect("restart tasks");

    let cmd_a = timeout(Duration::from_secs(2), rx_a.recv())
        .await
        .expect("timeout waiting for peer a")
        .expect("peer a command");
    let cmd_b = timeout(Duration::from_secs(2), rx_b.recv())
        .await
        .expect("timeout waiting for peer b")
        .expect("peer b command");

    assert_eq!(cmd_a, MessageCommand::GetData);
    assert_eq!(cmd_b, MessageCommand::GetData);

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn restart_tasks_without_sender_still_broadcasts_getdata() {
    let system = ActorSystem::new("task-manager-restart-no-sender").expect("actor system");
    let task_manager = system
        .actor_of(Props::new(TaskManagerActor::default), "task-manager")
        .expect("task manager actor");

    let (tx_a, mut rx_a) = mpsc::unbounded_channel();
    let actor_a = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx_a.clone() }),
            "peer-a",
        )
        .expect("peer a");

    let version = VersionPayload::default();
    task_manager
        .tell_from(
            TaskManagerCommand::Register {
                version: version.clone(),
            },
            Some(actor_a.clone()),
        )
        .expect("register a");

    let hash = UInt256::from([10u8; 32]);
    task_manager
        .tell(TaskManagerCommand::RestartTasks {
            payload: InvPayload::create(InventoryType::Transaction, &[hash]),
        })
        .expect("restart tasks");

    let cmd_a = timeout(Duration::from_secs(2), rx_a.recv())
        .await
        .expect("timeout waiting for peer a")
        .expect("peer a command");

    assert_eq!(cmd_a, MessageCommand::GetData);

    system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn new_extensible_tasks_from_registered_peer_request_getdata() {
    let neo_system = NeoSystem::new(
        neo_core::protocol_settings::ProtocolSettings::default(),
        None,
        None,
    )
    .expect("neo system");
    let system = ActorSystem::new("task-manager-extensible-new-tasks").expect("actor system");
    let task_manager = neo_system.task_manager_actor();

    let (tx, mut rx) = mpsc::unbounded_channel();
    let actor = system
        .actor_of(
            Props::new(move || CaptureActor { tx: tx.clone() }),
            "peer-extensible",
        )
        .expect("peer actor");

    let version = VersionPayload::create(
        &neo_core::protocol_settings::ProtocolSettings::default(),
        42,
        "/peer".to_string(),
        vec![],
    );
    task_manager
        .tell_from(
            TaskManagerCommand::Register { version },
            Some(actor.clone()),
        )
        .expect("register peer");

    let hash = UInt256::from([0x2e_u8; 32]);
    task_manager
        .tell_from(
            TaskManagerCommand::NewTasks {
                payload: InvPayload::create(InventoryType::Extensible, &[hash]),
            },
            Some(actor),
        )
        .expect("new tasks");

    let getdata = timeout(Duration::from_secs(2), async {
        loop {
            match rx.recv().await {
                Some(MessageCommand::GetData) => break MessageCommand::GetData,
                Some(_) => continue,
                None => panic!("peer channel closed before getdata"),
            }
        }
    })
    .await
    .expect("timeout waiting for getdata");

    assert_eq!(getdata, MessageCommand::GetData);

    system.shutdown().await.expect("shutdown");
    neo_system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn register_peer_requests_headers_with_default_count_sentinel() {
    let neo_system = NeoSystem::new(
        neo_core::protocol_settings::ProtocolSettings::default(),
        None,
        None,
    )
    .expect("neo system");
    let system = ActorSystem::new("task-manager-header-sentinel").expect("actor system");
    let task_manager = neo_system.task_manager_actor();

    let (tx, mut rx) = mpsc::unbounded_channel();
    let actor = system
        .actor_of(
            Props::new(move || MessageCaptureActor { tx: tx.clone() }),
            "peer-headers",
        )
        .expect("peer actor");

    let version = VersionPayload::create(
        &neo_core::protocol_settings::ProtocolSettings::default(),
        77,
        "/peer".to_string(),
        vec![neo_core::network::p2p::capabilities::NodeCapability::FullNode { start_height: 5 }],
    );
    task_manager
        .tell_from(TaskManagerCommand::Register { version }, Some(actor))
        .expect("register peer");

    let message = timeout(Duration::from_secs(2), async {
        loop {
            match rx.recv().await {
                Some(message) if message.command() == MessageCommand::GetHeaders => break message,
                Some(_) => continue,
                None => panic!("peer channel closed before getheaders"),
            }
        }
    })
    .await
    .expect("timeout waiting for getheaders");

    match message.payload {
        ProtocolMessage::GetHeaders(payload) => assert_eq!(payload.count, -1),
        other => panic!("expected getheaders payload, got {other:?}"),
    }

    let no_extra = timeout(Duration::from_millis(300), rx.recv()).await;
    assert!(
        no_extra.is_err(),
        "header request pass should not also enqueue block-by-index immediately"
    );

    system.shutdown().await.expect("shutdown");
    neo_system.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn register_closed_peer_does_not_emit_task_manager_warnings() {
    let warning_count = Arc::new(AtomicUsize::new(0));
    let subscriber = tracing_subscriber::registry().with(WarnCountLayer {
        count: warning_count.clone(),
    });
    let dispatch = tracing::Dispatch::new(subscriber);
    let _guard = tracing::dispatcher::set_default(&dispatch);

    let system = ActorSystem::new("task-manager-dead-peer").expect("actor system");
    let task_manager = system
        .actor_of(Props::new(TaskManagerActor::default), "task-manager")
        .expect("task manager actor");

    let dead_actor = system
        .actor_of(
            Props::new(move || MessageCaptureActor {
                tx: mpsc::unbounded_channel().0,
            }),
            "dead-peer",
        )
        .expect("dead peer actor");

    dead_actor.stop().expect("stop dead peer");
    timeout(Duration::from_secs(2), async {
        loop {
            let result = dead_actor.tell(RemoteNodeCommand::Send(NetworkMessage::new(
                ProtocolMessage::Mempool,
            )));
            if result.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("dead peer should stop");

    task_manager
        .tell_from(
            TaskManagerCommand::Register {
                version: VersionPayload::default(),
            },
            Some(dead_actor),
        )
        .expect("register dead peer");

    tokio::time::sleep(Duration::from_millis(100)).await;

    assert_eq!(
        warning_count.load(Ordering::SeqCst),
        0,
        "registering a closed peer should be ignored without warning noise",
    );

    system.shutdown().await.expect("shutdown");
}
