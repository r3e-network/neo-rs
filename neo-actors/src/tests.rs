use super::mailbox::DefaultMailbox;
use super::*;
use async_trait::async_trait;
use parking_lot::Mutex;
use std::any::Any;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;
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
                CounterMsg::Fail => Err(ActorRuntimeError::actor("boom")),
            },
            Err(_) => Err(ActorRuntimeError::actor("unexpected message")),
        }
    }

    async fn on_failure(
        &mut self,
        _ctx: &mut ActorContext,
        _error: &ActorRuntimeError,
    ) -> SupervisorDirective {
        SupervisorDirective::Restart
    }
}

async fn counter_setup(system_name: &str) -> ActorRuntimeResult<(ActorSystem, ActorRef)> {
    let system = ActorSystem::new(system_name.to_string())?;
    let counter = system.actor_of(Props::new(CounterActor::default), "counter")?;
    Ok((system, counter))
}

#[tokio::test]
async fn counter_tell_and_ask() -> ActorRuntimeResult<()> {
    let (system, counter) = counter_setup("actor-runtime-counter").await?;

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
async fn counter_restart_on_failure() -> ActorRuntimeResult<()> {
    let (system, counter) = counter_setup("actor-runtime-restart").await?;

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
async fn actor_selection_returns_ref() -> ActorRuntimeResult<()> {
    let (system, counter) = counter_setup("actor-runtime-selection").await?;

    let selection_path = format!("/{}/user/counter", system.name());
    let selected = system
        .actor_selection(&selection_path)
        .expect("actor selection to succeed");

    assert_eq!(selected.path(), counter.path());

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn actor_of_rejects_duplicate_name() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-duplicate-name")?;

    let first = system.actor_of(Props::new(CounterActor::default), "counter")?;
    let err = system
        .actor_of(Props::new(CounterActor::default), "counter")
        .expect_err("duplicate actor name should fail");

    assert!(
        matches!(err, ActorRuntimeError::System(message) if message.contains(&first.path().to_string()))
    );

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
                Err(_) => Err(ActorRuntimeError::actor("unexpected message")),
            },
        }
    }
}

#[tokio::test]
async fn watch_sends_terminated() -> ActorRuntimeResult<()> {
    let (system, counter) = counter_setup("actor-runtime-watch").await?;

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
        .map_err(|_| ActorRuntimeError::AskTimeout)?
        .map_err(|_| ActorRuntimeError::AskTimeout)?;

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
            Err(ActorRuntimeError::actor("unexpected message"))
        }
    }
}

#[tokio::test]
async fn event_stream_publish_delivers_messages() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-event-stream")?;
    let (tx, rx) = oneshot::channel();
    let holder = Arc::new(Mutex::new(Some(tx)));
    let holder_clone = holder.clone();

    let probe = system.actor_of(
        Props::new(move || EventProbe {
            notify: holder_clone.lock().take(),
        }),
        "probe",
    )?;

    let event_stream = system.event_stream();
    event_stream.subscribe::<TestEvent>(probe.clone());

    event_stream.publish(TestEvent(42));

    let value = timeout(Duration::from_millis(200), rx)
        .await
        .map_err(|_| ActorRuntimeError::AskTimeout)?
        .map_err(|_| ActorRuntimeError::AskTimeout)?;

    assert_eq!(value, 42);

    event_stream.unsubscribe_all(&probe);
    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn event_stream_subscribe_channel_delivers_messages() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-event-stream-channel")?;
    let event_stream = system.event_stream();
    let mut rx = event_stream.subscribe_channel::<TestEvent>();

    event_stream.publish(TestEvent(7));

    let value = timeout(Duration::from_millis(200), rx.recv())
        .await
        .map_err(|_| ActorRuntimeError::AskTimeout)?
        .expect("channel subscriber should receive the published event");

    assert_eq!(value.0, 7);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn event_stream_channel_subscribers_each_receive_a_copy() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-event-stream-channel-fanout")?;
    let event_stream = system.event_stream();
    let mut rx1 = event_stream.subscribe_channel::<TestEvent>();
    let mut rx2 = event_stream.subscribe_channel::<TestEvent>();

    event_stream.publish(TestEvent(11));

    let v1 = timeout(Duration::from_millis(200), rx1.recv())
        .await
        .map_err(|_| ActorRuntimeError::AskTimeout)?
        .expect("first channel subscriber");
    let v2 = timeout(Duration::from_millis(200), rx2.recv())
        .await
        .map_err(|_| ActorRuntimeError::AskTimeout)?
        .expect("second channel subscriber");

    assert_eq!(v1.0, 11);
    assert_eq!(v2.0, 11);

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn event_stream_channel_subscriber_pruned_after_receiver_dropped() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-event-stream-channel-prune")?;
    let event_stream = system.event_stream();

    // Drop the receiver, then publish: the dead sender must be pruned without
    // panicking. A fresh subscriber registered afterwards still receives events.
    let rx = event_stream.subscribe_channel::<TestEvent>();
    drop(rx);
    event_stream.publish(TestEvent(1));

    let mut rx2 = event_stream.subscribe_channel::<TestEvent>();
    event_stream.publish(TestEvent(2));

    let value = timeout(Duration::from_millis(200), rx2.recv())
        .await
        .map_err(|_| ActorRuntimeError::AskTimeout)?
        .expect("fresh channel subscriber should receive the event");
    assert_eq!(value.0, 2);

    system.shutdown().await?;
    Ok(())
}

struct StopProbe {
    stopped: Option<oneshot::Sender<()>>,
}

#[async_trait]
impl Actor for StopProbe {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        drop(message);
        Ok(())
    }

    async fn post_stop(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        if let Some(stopped) = self.stopped.take() {
            let _ = stopped.send(());
        }
        Ok(())
    }
}

#[tokio::test]
async fn shutdown_stops_and_waits_for_user_actors() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-shutdown-waits")?;
    let (stopped_tx, stopped_rx) = oneshot::channel();
    let stopped_holder = Arc::new(Mutex::new(Some(stopped_tx)));
    let actor_holder = stopped_holder.clone();

    let actor = system.actor_of(
        Props::new(move || StopProbe {
            stopped: actor_holder.lock().take(),
        }),
        "probe",
    )?;
    let actor_path = actor.path().to_string();

    system.shutdown().await?;
    timeout(Duration::from_millis(500), stopped_rx)
        .await
        .map_err(|_| ActorRuntimeError::AskTimeout)?
        .map_err(|_| ActorRuntimeError::AskTimeout)?;

    assert!(system.actor_selection(&actor_path).is_none());
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
            Err(ActorRuntimeError::actor("unexpected message"))
        }
    }
}

#[tokio::test]
async fn scheduler_repeated_messages_can_be_cancelled() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-scheduler")?;
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
    assert!(
        value_after_cancel > 0,
        "retained schedule handle should allow repeated messages to fire"
    );
    sleep(Duration::from_millis(30)).await;
    assert_eq!(value_after_cancel, counter.load(Ordering::SeqCst));

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn scheduler_once_message_can_be_cancelled() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-scheduler-once-cancel")?;
    let counter = Arc::new(AtomicUsize::new(0));
    let actor_counter = counter.clone();
    let ticker = system.actor_of(
        Props::new(move || TickActor {
            counter: actor_counter.clone(),
        }),
        "ticker",
    )?;

    let scheduler = system.scheduler();
    let handle =
        scheduler.schedule_tell_once(Duration::from_millis(30), ticker.clone(), Tick, None);
    handle.cancel();

    sleep(Duration::from_millis(60)).await;
    assert_eq!(
        0,
        counter.load(Ordering::SeqCst),
        "cancelled one-shot schedules should not deliver their message"
    );

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn scheduler_repeated_messages_stop_when_handle_is_dropped() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-scheduler-drop")?;
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
    drop(handle);

    sleep(Duration::from_millis(30)).await;
    assert_eq!(
        0,
        counter.load(Ordering::SeqCst),
        "dropping the schedule handle should cancel repeated messages"
    );

    system.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn dropping_cloned_schedule_handle_cancels_repeated_messages() -> ActorRuntimeResult<()> {
    let system = ActorSystem::new("actor-runtime-scheduler-drop-clone")?;
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
        Duration::from_millis(50),
        Duration::from_millis(5),
        ticker.clone(),
        Tick,
        None,
    );
    let cloned = handle.clone();
    drop(cloned);

    sleep(Duration::from_millis(80)).await;
    assert_eq!(
        0,
        counter.load(Ordering::SeqCst),
        "dropping any cloned schedule handle should cancel repeated messages"
    );

    system.shutdown().await?;
    Ok(())
}

#[test]
fn default_mailbox_prioritizes_system_messages() {
    let mut mailbox = DefaultMailbox::default();

    mailbox.enqueue(MailboxMessage::User(Envelope::new("first", None)));
    mailbox.enqueue(MailboxMessage::System(SystemMessage::Stop));
    mailbox.enqueue(MailboxMessage::User(Envelope::new("second", None)));

    assert!(matches!(
        mailbox.dequeue(),
        Some(MailboxMessage::System(SystemMessage::Stop))
    ));

    let first = mailbox.dequeue().expect("first user message");
    assert_eq!(
        first.as_user().and_then(|env| env.downcast_ref::<&str>()),
        Some(&"first")
    );

    let second = mailbox.dequeue().expect("second user message");
    assert_eq!(
        second.as_user().and_then(|env| env.downcast_ref::<&str>()),
        Some(&"second")
    );
    assert!(mailbox.is_empty());
}

#[tokio::test]
async fn task_executor_cancels_and_awaits_tasks() {
    let executor = TaskExecutor::new();
    let stopped = Arc::new(AtomicUsize::new(0));
    let stopped_clone = stopped.clone();
    executor.spawn(move |token| async move {
        token.cancelled().await;
        stopped_clone.store(1, Ordering::SeqCst);
    });

    assert!(!executor.is_shutting_down());
    executor.shutdown().await;
    assert!(executor.is_shutting_down());
    assert_eq!(
        stopped.load(Ordering::SeqCst),
        1,
        "task must observe cooperative cancellation"
    );
}

#[tokio::test]
async fn task_executor_stops_select_loop_on_shutdown() {
    let executor = TaskExecutor::new();
    let ticks = Arc::new(AtomicUsize::new(0));
    let ticks_clone = ticks.clone();
    executor.spawn(move |token| async move {
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = sleep(Duration::from_millis(1)) => {
                    ticks_clone.fetch_add(1, Ordering::SeqCst);
                }
            }
        }
    });

    sleep(Duration::from_millis(5)).await;
    executor.shutdown().await;
    let after = ticks.load(Ordering::SeqCst);
    sleep(Duration::from_millis(5)).await;
    assert_eq!(
        after,
        ticks.load(Ordering::SeqCst),
        "the select! loop must stop incrementing after shutdown"
    );
}
