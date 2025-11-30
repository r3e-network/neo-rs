use super::*;
use crate::supervision::SupervisorDirective;
use async_trait::async_trait;
use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{oneshot, Mutex as AsyncMutex};
use tokio::time::{sleep, timeout, Duration};

#[derive(Default)]
struct CounterActor {
    count: u32,
}

enum CounterMsg {
    Add(u32),
    Get(oneshot::Sender<u32>),
    Fail,
}

#[async_trait]
impl Actor for CounterActor {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        match message.downcast::<CounterMsg>() {
            Ok(msg) => match *msg {
                CounterMsg::Add(value) => {
                    self.count += value;
                    Ok(())
                }
                CounterMsg::Get(sender) => {
                    let _ = sender.send(self.count);
                    Ok(())
                }
                CounterMsg::Fail => Err(AkkaError::actor("boom")),
            },
            Err(_) => Err(AkkaError::actor("unexpected message")),
        }
    }

    async fn on_failure(
        &mut self,
        _ctx: &mut ActorContext,
        _error: &AkkaError,
    ) -> SupervisorDirective {
        SupervisorDirective::Restart
    }
}

async fn counter_setup(system_name: &str) -> AkkaResult<(ActorSystem, ActorRef)> {
    let system = ActorSystem::new(system_name.to_string())?;
    let counter = system.actor_of(Props::new(CounterActor::default), "counter")?;
    Ok((system, counter))
}

#[tokio::test]
async fn counter_tell_and_ask() -> AkkaResult<()> {
    let (system, counter) = counter_setup("akka-counter").await?;

    counter.tell(CounterMsg::Add(5))?;
    sleep(Duration::from_millis(10)).await;

    let value = counter
        .ask(
            |reply| Box::new(CounterMsg::Get(reply)),
            Duration::from_millis(200),
        )
        .await?;

    assert_eq!(value, 5);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn counter_restart_on_failure() -> AkkaResult<()> {
    let (system, counter) = counter_setup("akka-restart").await?;

    counter.tell(CounterMsg::Add(10))?;
    sleep(Duration::from_millis(10)).await;

    counter.tell(CounterMsg::Fail)?;
    sleep(Duration::from_millis(10)).await;

    counter.tell(CounterMsg::Add(1))?;
    sleep(Duration::from_millis(10)).await;

    let value = counter
        .ask(
            |reply| Box::new(CounterMsg::Get(reply)),
            Duration::from_millis(200),
        )
        .await?;

    assert_eq!(value, 1);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn actor_selection_returns_ref() -> AkkaResult<()> {
    let (system, counter) = counter_setup("akka-selection").await?;

    let selection_path = format!("/{}/user/counter", system.name());
    let selected = system
        .actor_selection(&selection_path)
        .expect("actor selection to succeed");

    assert_eq!(selected.path(), counter.path());

    system.shutdown().await?;
    Ok(())
}

#[derive(Default)]
struct WatcherActor {
    notify: Option<oneshot::Sender<ActorRef>>,
}

struct WatchCommand {
    actor: ActorRef,
    notify: oneshot::Sender<ActorRef>,
}

#[async_trait]
impl Actor for WatcherActor {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match message.downcast::<WatchCommand>() {
            Ok(cmd) => {
                let WatchCommand { actor, notify } = *cmd;
                ctx.watch(&actor)?;
                self.notify = Some(notify);
                Ok(())
            }
            Err(message) => match message.downcast::<Terminated>() {
                Ok(terminated) => {
                    let terminated = *terminated;
                    if let Some(sender) = self.notify.take() {
                        let _ = sender.send(terminated.actor);
                    }
                    Ok(())
                }
                Err(_) => Err(AkkaError::actor("unexpected message")),
            },
        }
    }
}

#[tokio::test]
async fn watch_sends_terminated() -> AkkaResult<()> {
    let (system, counter) = counter_setup("akka-watch").await?;

    let (notify_tx, notify_rx) = oneshot::channel();
    let watcher = system.actor_of(Props::new(WatcherActor::default), "watcher")?;
    watcher.tell(WatchCommand {
        actor: counter.clone(),
        notify: notify_tx,
    })?;

    sleep(Duration::from_millis(25)).await;
    counter.stop()?;

    let terminated = timeout(Duration::from_millis(500), notify_rx)
        .await
        .map_err(|_| AkkaError::AskTimeout)?
        .map_err(|_| AkkaError::AskTimeout)?;

    assert_eq!(terminated.path(), counter.path());
    system.shutdown().await?;
    Ok(())
}

#[derive(Clone)]
struct TestEvent(pub u32);

#[derive(Default)]
struct EventProbe {
    notify: Option<oneshot::Sender<u32>>,
}

#[async_trait]
impl Actor for EventProbe {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(event) = message.downcast::<TestEvent>() {
            let TestEvent(value) = *event;
            if let Some(sender) = self.notify.take() {
                let _ = sender.send(value);
            }
            Ok(())
        } else {
            Err(AkkaError::actor("unexpected message"))
        }
    }
}

#[tokio::test]
async fn event_stream_publish_delivers_messages() -> AkkaResult<()> {
    let system = ActorSystem::new("akka-event-stream")?;
    let (tx, rx) = oneshot::channel();
    let holder = Arc::new(Mutex::new(Some(tx)));
    let holder_clone = holder.clone();

    let probe = system.actor_of(
        Props::new(move || EventProbe {
            notify: holder_clone.lock().unwrap().take(),
        }),
        "probe",
    )?;

    let event_stream = system.event_stream();
    event_stream.subscribe::<TestEvent>(probe.clone());

    event_stream.publish(TestEvent(42));

    let value = timeout(Duration::from_millis(200), rx)
        .await
        .map_err(|_| AkkaError::AskTimeout)?
        .map_err(|_| AkkaError::AskTimeout)?;

    assert_eq!(value, 42);

    event_stream.unsubscribe_all(&probe);
    system.shutdown().await?;
    Ok(())
}

#[derive(Clone)]
struct Tick;

struct TickActor {
    counter: Arc<AtomicUsize>,
}

#[async_trait]
impl Actor for TickActor {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        if message.downcast::<Tick>().is_ok() {
            self.counter.fetch_add(1, Ordering::SeqCst);
            Ok(())
        } else {
            Err(AkkaError::actor("unexpected message"))
        }
    }
}

#[tokio::test]
async fn scheduler_repeated_messages_can_be_cancelled() -> AkkaResult<()> {
    let system = ActorSystem::new("akka-scheduler")?;
    let counter = Arc::new(AtomicUsize::new(0));
    let actor_counter = counter.clone();
    let ticker = system.actor_of(
        Props::new(move || TickActor {
            counter: actor_counter.clone(),
        }),
        "ticker",
    )?;

    let scheduler = system.scheduler();
    let handle = scheduler.schedule_tell_repeatedly(
        Duration::from_millis(5),
        Duration::from_millis(5),
        ticker.clone(),
        Tick,
        None,
    );

    sleep(Duration::from_millis(30)).await;
    handle.cancel();

    let value_after_cancel = counter.load(Ordering::SeqCst);
    sleep(Duration::from_millis(30)).await;
    assert_eq!(value_after_cancel, counter.load(Ordering::SeqCst));

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn inbox_collects_messages() -> AkkaResult<()> {
    let (system, counter) = counter_setup("akka-inbox").await?;
    let inbox = Inbox::create(&system)?;

    inbox.watch(&counter)?;
    counter.stop()?;

    let message = inbox
        .receive(Duration::from_millis(500))
        .await
        .map_err(|_| AkkaError::AskTimeout)?;

    let terminated = *message
        .downcast::<Terminated>()
        .map_err(|_| AkkaError::actor("unexpected inbox message"))?;

    assert_eq!(terminated.actor.path(), counter.path());

    system.shutdown().await?;
    Ok(())
}

#[derive(Clone)]
enum PriorityMsg {
    High(u32),
    Low(u32),
}

struct PriorityActor {
    log: Arc<AsyncMutex<Vec<String>>>,
}

#[async_trait]
impl Actor for PriorityActor {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(msg) = message.downcast::<PriorityMsg>() {
            let label = match *msg {
                PriorityMsg::High(v) => format!("high-{v}"),
                PriorityMsg::Low(v) => format!("low-{v}"),
            };
            self.log.lock().await.push(label);
            Ok(())
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn priority_mailbox_delivers_high_priority_first() -> AkkaResult<()> {
    let system = ActorSystem::new("akka-priority")?;
    let log = Arc::new(AsyncMutex::new(Vec::new()));
    let log_clone = log.clone();

    let props = Props::new(move || PriorityActor {
        log: log_clone.clone(),
    })
    .with_priority_mailbox(PriorityMailboxConfig::default().with_priority(|message| {
        message
            .as_user()
            .and_then(|env| env.downcast_ref::<PriorityMsg>())
            .map(|msg| matches!(msg, PriorityMsg::High(_)))
            .unwrap_or(false)
    }));

    let actor = system.actor_of(props, "priority")?;
    let actor_high = actor.clone();
    let actor_low_second = actor.clone();

    tokio::join!(
        async {
            actor.tell(PriorityMsg::Low(1)).unwrap();
        },
        async {
            actor_high.tell(PriorityMsg::High(3)).unwrap();
        },
        async {
            actor_low_second.tell(PriorityMsg::Low(2)).unwrap();
        },
    );

    sleep(Duration::from_millis(30)).await;

    let entries = log.lock().await.clone();
    assert_eq!(entries.len(), 3);
    let index_high = entries
        .iter()
        .position(|entry| entry == "high-3")
        .expect("high priority message processed");
    let index_low2 = entries
        .iter()
        .position(|entry| entry == "low-2")
        .expect("second low priority message processed");
    assert!(index_high < index_low2);

    system.shutdown().await?;
    Ok(())
}

#[derive(Clone)]
struct DuplicateMsg(u32);

#[test]
fn priority_mailbox_drops_duplicates() {
    let config = PriorityMailboxConfig::default().with_dropper(|message, view| {
        let incoming = match message
            .as_user()
            .and_then(|env| env.downcast_ref::<DuplicateMsg>())
        {
            Some(value) => value,
            None => return false,
        };

        view.iter().any(|existing| {
            existing
                .as_user()
                .and_then(|env| env.downcast_ref::<DuplicateMsg>())
                .map(|queued| queued.0 == incoming.0)
                .unwrap_or(false)
        })
    });

    let mut mailbox = PriorityMailbox::new(config);

    mailbox.enqueue(MailboxMessage::User(Envelope::new(DuplicateMsg(1), None)));
    mailbox.enqueue(MailboxMessage::User(Envelope::new(DuplicateMsg(1), None)));
    mailbox.enqueue(MailboxMessage::User(Envelope::new(DuplicateMsg(2), None)));

    let first = mailbox.dequeue().expect("first message");
    let second = mailbox.dequeue().expect("second message");
    let third = mailbox.dequeue();
    let fourth = mailbox.dequeue();

    let extract = |message: MailboxMessage| -> DuplicateMsg {
        match message {
            MailboxMessage::User(envelope) => *envelope
                .message
                .downcast::<DuplicateMsg>()
                .expect("duplicate message"),
            MailboxMessage::System(_) => panic!("unexpected system message"),
        }
    };

    let first_msg = extract(first);
    let second_msg = extract(second);

    assert_eq!(first_msg.0, 1);
    assert_eq!(second_msg.0, 2);
    match third {
        Some(MailboxMessage::User(envelope)) => {
            assert!(envelope.is::<Idle>());
        }
        _ => panic!("expected idle envelope"),
    }
    assert!(fourth.is_none());
}

struct IdleActor {
    notify: Arc<AsyncMutex<Option<oneshot::Sender<()>>>>,
}

#[async_trait]
impl Actor for IdleActor {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        if message.downcast::<Idle>().is_ok() {
            if let Some(sender) = self.notify.lock().await.take() {
                let _ = sender.send(());
            }
        }
        Ok(())
    }
}

#[tokio::test]
async fn priority_mailbox_emits_idle_signal() -> AkkaResult<()> {
    let system = ActorSystem::new("akka-idle")?;
    let (tx, rx) = oneshot::channel();
    let notify = Arc::new(AsyncMutex::new(Some(tx)));

    let _actor = system.actor_of(
        Props::new({
            let notify = notify.clone();
            move || IdleActor {
                notify: notify.clone(),
            }
        })
        .with_priority_mailbox(PriorityMailboxConfig::default()),
        "idle",
    )?;

    timeout(Duration::from_millis(200), rx)
        .await
        .map_err(|_| AkkaError::AskTimeout)?
        .map_err(|_| AkkaError::AskTimeout)?;

    system.shutdown().await?;
    Ok(())
}

#[derive(Default)]
struct FragileActor;

#[async_trait]
impl Actor for FragileActor {
    async fn handle(
        &mut self,
        _message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        Err(AkkaError::actor("boom"))
    }
}

#[tokio::test]
async fn supervisor_strategy_enforces_restart_budget() -> AkkaResult<()> {
    let system = ActorSystem::new("akka-restart-budget")?;

    let props = Props::new(FragileActor::default).with_strategy(SupervisorStrategy::one_for_one(
        Some(1),
        Some(Duration::from_millis(200)),
        |_| SupervisorDirective::Restart,
    ));

    let actor = system.actor_of(props, "fragile")?;
    let inbox = Inbox::create(&system)?;
    inbox.watch(&actor)?;

    actor.tell(())?;
    sleep(Duration::from_millis(20)).await;
    actor.tell(())?;

    let message = inbox
        .receive(Duration::from_millis(500))
        .await
        .map_err(|_| AkkaError::AskTimeout)?;

    let terminated = *message
        .downcast::<Terminated>()
        .map_err(|_| AkkaError::actor("expected termination notification"))?;

    assert_eq!(terminated.actor.path(), actor.path());

    system.shutdown().await?;
    Ok(())
}
