use crate::{
    actor::Actor,
    actor_ref::ActorRef,
    context::ActorContext,
    error::{AkkaError, AkkaResult},
    event_stream::{EventStream, EventStreamHandle},
    mailbox::{default_mailbox_factory, Mailbox},
    message::{MailboxMessage, SystemMessage, Terminated},
    props::Props,
    scheduler::Scheduler,
    supervision::{FailureTracker, SupervisorDirective, SupervisorStrategy},
};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::{any::Any, collections::HashMap, fmt, sync::Arc};
use tokio::sync::mpsc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ActorPath {
    system: String,
    segments: Vec<String>,
}

impl ActorPath {
    pub fn new(system: impl Into<String>, segments: Vec<String>) -> Self {
        Self {
            system: system.into(),
            segments,
        }
    }

    pub fn root(system: impl Into<String>, name: impl Into<String>) -> Self {
        Self {
            system: system.into(),
            segments: vec![name.into()],
        }
    }

    pub fn child(&self, name: impl Into<String>) -> Self {
        let mut segments = self.segments.clone();
        segments.push(name.into());
        Self {
            system: self.system.clone(),
            segments,
        }
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    pub fn parse(path: &str) -> Option<Self> {
        let mut parts = path.split('/').filter(|p| !p.is_empty());
        let system = parts.next()?.to_string();
        let segments: Vec<String> = parts.map(|p| p.to_string()).collect();
        if segments.is_empty() {
            return None;
        }
        Some(Self { system, segments })
    }
}

impl fmt::Display for ActorPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}/{}", self.system, self.segments.join("/"))
    }
}

pub(crate) enum MailboxCommand {
    Message(MailboxMessage),
}

struct ActorEntry {
    mailbox: mpsc::UnboundedSender<MailboxCommand>,
}

pub(crate) struct ActorSystemInner {
    pub name: String,
    registry: RwLock<HashMap<String, ActorEntry>>,
    runtime: tokio::runtime::Handle,
    event_stream: Arc<EventStream>,
}

impl ActorSystemInner {
    fn new(name: String) -> AkkaResult<Arc<Self>> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| AkkaError::system("Akka actor system requires a Tokio runtime"))?;

        Ok(Arc::new(Self {
            name,
            registry: RwLock::new(HashMap::new()),
            runtime,
            event_stream: EventStream::new(),
        }))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn scheduler(&self) -> Scheduler {
        Scheduler::new(self.runtime.clone())
    }

    pub fn event_stream(&self) -> EventStreamHandle {
        EventStreamHandle::new(Arc::clone(&self.event_stream))
    }

    pub fn spawn_root(
        self: &Arc<Self>,
        props: Props,
        name: impl Into<String>,
    ) -> AkkaResult<ActorRef> {
        let path = ActorPath::root(self.name.clone(), name.into());
        self.spawn_actor_internal(None, props, path)
    }

    pub fn spawn_child(
        self: &Arc<Self>,
        parent: ActorRef,
        props: Props,
        name: Option<String>,
    ) -> AkkaResult<ActorRef> {
        let child_name = name.unwrap_or_else(ActorRef::unique_child_name);
        let path = parent.path().child(child_name);
        self.spawn_actor_internal(Some(parent), props, path)
    }

    fn spawn_actor_internal(
        self: &Arc<Self>,
        parent: Option<ActorRef>,
        props: Props,
        path: ActorPath,
    ) -> AkkaResult<ActorRef> {
        let (tx, rx) = mpsc::unbounded_channel();

        {
            let mut registry = self.registry.write();
            let key = path.to_string();
            if registry.contains_key(&key) {
                return Err(AkkaError::system(format!("Actor {} already exists", path)));
            }
            registry.insert(
                key,
                ActorEntry {
                    mailbox: tx.clone(),
                },
            );
        }

        let system = Arc::clone(self);
        let actor_ref = ActorRef::new(path.clone(), tx.clone(), Arc::downgrade(self));
        let actor = props.create();
        let mailbox = props.create_mailbox();
        let cell = ActorCell::new(system, actor, props, mailbox, rx, actor_ref.clone(), parent);

        self.runtime.spawn(async move {
            cell.run().await;
        });

        Ok(actor_ref)
    }

    fn unregister(&self, path: &ActorPath) {
        let mut registry = self.registry.write();
        registry.remove(&path.to_string());
    }

    pub fn resolve(self: &Arc<Self>, path: &ActorPath) -> Option<ActorRef> {
        let registry = self.registry.read();
        let entry = registry.get(&path.to_string())?;
        Some(ActorRef::new(
            path.clone(),
            entry.mailbox.clone(),
            Arc::downgrade(self),
        ))
    }
}

/// Public entry point for spawning and supervising actors.
pub struct ActorSystem {
    inner: Arc<ActorSystemInner>,
    user_guardian: ActorRef,
}

impl ActorSystem {
    pub fn new(name: impl Into<String>) -> AkkaResult<Self> {
        let name = name.into();
        let inner = ActorSystemInner::new(name.clone())?;
        let guardian_props =
            Props::new(|| Guardian::default()).with_mailbox_factory(default_mailbox_factory());
        let user_guardian = inner.clone().spawn_root(guardian_props, "user")?;

        Ok(Self {
            inner,
            user_guardian,
        })
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub fn actor_of(&self, props: Props, name: impl Into<String>) -> AkkaResult<ActorRef> {
        self.inner
            .clone()
            .spawn_child(self.user_guardian.clone(), props, Some(name.into()))
    }

    pub fn actor_selection(&self, path: &str) -> Option<ActorRef> {
        let parsed = ActorPath::parse(path)?;
        if parsed.system != self.name() {
            return None;
        }
        self.inner.clone().resolve(&parsed)
    }

    pub fn stop(&self, actor: &ActorRef) -> AkkaResult<()> {
        actor.stop()
    }

    pub fn scheduler(&self) -> Scheduler {
        self.inner.scheduler()
    }

    pub fn event_stream(&self) -> EventStreamHandle {
        self.inner.event_stream()
    }

    pub fn handle(&self) -> ActorSystemHandle {
        ActorSystemHandle::new(self.inner.clone())
    }

    pub async fn shutdown(self) -> AkkaResult<()> {
        self.user_guardian.stop()?;
        Ok(())
    }

    pub fn guardian(&self) -> ActorRef {
        self.user_guardian.clone()
    }
}

struct ActorCell {
    system: Arc<ActorSystemInner>,
    actor: Box<dyn Actor>,
    props: Props,
    mailbox: Box<dyn Mailbox>,
    commands: mpsc::UnboundedReceiver<MailboxCommand>,
    self_ref: ActorRef,
    parent: Option<ActorRef>,
    strategy: Option<SupervisorStrategy>,
    failures: FailureTracker,
    watchers: Vec<ActorRef>,
}

impl ActorCell {
    fn new(
        system: Arc<ActorSystemInner>,
        actor: Box<dyn Actor>,
        props: Props,
        mailbox: Box<dyn Mailbox>,
        commands: mpsc::UnboundedReceiver<MailboxCommand>,
        self_ref: ActorRef,
        parent: Option<ActorRef>,
    ) -> Self {
        Self {
            strategy: props.strategy.clone(),
            system,
            actor,
            props,
            mailbox,
            commands,
            self_ref,
            parent,
            failures: FailureTracker::new(),
            watchers: Vec::new(),
        }
    }

    async fn run(mut self) {
        let mut context = ActorContext {
            system: self.system.clone(),
            self_ref: self.self_ref.clone(),
            parent: self.parent.clone(),
            sender: None,
            children: Vec::new(),
        };

        if let Err(err) = self.actor.pre_start(&mut context).await {
            if !self.handle_failure(err, &mut context).await {
                self.cleanup(&mut context).await;
                return;
            }
        }

        loop {
            if let Some(message) = self.mailbox.dequeue() {
                if self.process_message(message, &mut context).await {
                    break;
                }
                continue;
            }

            match self.commands.recv().await {
                Some(MailboxCommand::Message(message)) => {
                    self.mailbox.enqueue(message);
                }
                None => {
                    break;
                }
            }
        }

        self.cleanup(&mut context).await;
    }

    async fn process_message(
        &mut self,
        message: MailboxMessage,
        context: &mut ActorContext,
    ) -> bool {
        match message {
            MailboxMessage::User(envelope) => {
                let (msg, sender) = envelope.take();
                context.sender = sender;
                if let Err(err) = self.actor.handle(msg, context).await {
                    if !self.handle_failure(err, context).await {
                        return true;
                    }
                }
                false
            }
            MailboxMessage::System(system_msg) => match system_msg {
                SystemMessage::Stop => true,
                SystemMessage::Suspend => false,
                SystemMessage::Resume => false,
                SystemMessage::Watch(watcher) => {
                    if watcher != self.self_ref
                        && !self.watchers.iter().any(|existing| existing == &watcher)
                    {
                        self.watchers.push(watcher);
                    }
                    false
                }
                SystemMessage::Unwatch(watcher) => {
                    self.watchers.retain(|existing| existing != &watcher);
                    false
                }
            },
        }
    }

    async fn cleanup(&mut self, context: &mut ActorContext) {
        let _ = self.actor.post_stop(context).await;
        for child in context.children.drain(..) {
            let _ = child.stop();
        }
        let path = self.self_ref.path();
        self.system.unregister(&path);

        let terminated = Terminated::new(self.self_ref.clone());
        for watcher in self.watchers.drain(..) {
            let _ = watcher.tell_from(terminated.clone(), Some(self.self_ref.clone()));
        }
    }

    async fn handle_failure(&mut self, error: AkkaError, ctx: &mut ActorContext) -> bool {
        let directive = if let Some(strategy) = &self.strategy {
            strategy.decide(&error, &mut self.failures)
        } else {
            self.actor.on_failure(ctx, &error).await
        };
        match directive {
            SupervisorDirective::Stop(_) | SupervisorDirective::Escalate => false,
            SupervisorDirective::Resume => true,
            SupervisorDirective::Restart => {
                let _ = self.actor.post_stop(ctx).await;
                self.actor = self.props.create();
                ctx.sender = None;
                if let Err(_err) = self.actor.pre_start(ctx).await {
                    false
                } else {
                    true
                }
            }
        }
    }
}

#[derive(Default)]
struct Guardian;

#[async_trait]
impl Actor for Guardian {
    async fn handle(
        &mut self,
        message: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> crate::actor::ActorResult {
        drop(message);
        Ok(())
    }
}

#[derive(Clone)]
pub struct ActorSystemHandle {
    inner: Arc<ActorSystemInner>,
}

impl ActorSystemHandle {
    pub(crate) fn new(inner: Arc<ActorSystemInner>) -> Self {
        Self { inner }
    }

    pub fn name(&self) -> &str {
        self.inner.name()
    }

    pub fn scheduler(&self) -> Scheduler {
        self.inner.scheduler()
    }

    pub fn event_stream(&self) -> EventStreamHandle {
        self.inner.event_stream()
    }
}
