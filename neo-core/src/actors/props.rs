use super::{
    actor::Actor,
    mailbox::{
        default_mailbox_factory, mailbox_factory_from, priority_mailbox_factory, Mailbox,
        MailboxFactory, PriorityMailboxConfig,
    },
};
use std::{fmt, sync::Arc};

/// Actor factory used when spawning a new instance.
pub struct Props {
    factory: Arc<dyn Fn() -> Box<dyn Actor> + Send + Sync>,
    pub(super) strategy: Option<super::supervision::SupervisorStrategy>,
    mailbox_factory: MailboxFactory,
}

impl Props {
    pub fn new<F, A>(factory: F) -> Self
    where
        F: Fn() -> A + Send + Sync + 'static,
        A: Actor,
    {
        Self {
            factory: Arc::new(move || Box::new(factory()) as Box<dyn Actor>),
            strategy: None,
            mailbox_factory: default_mailbox_factory(),
        }
    }

    pub fn with_strategy(mut self, strategy: super::supervision::SupervisorStrategy) -> Self {
        self.strategy = Some(strategy);
        self
    }

    pub fn with_mailbox_factory(mut self, factory: MailboxFactory) -> Self {
        self.mailbox_factory = factory;
        self
    }

    pub fn with_priority_mailbox(mut self, config: PriorityMailboxConfig) -> Self {
        self.mailbox_factory = priority_mailbox_factory(config);
        self
    }

    pub(crate) fn create(&self) -> Box<dyn Actor> {
        (self.factory)()
    }

    pub(crate) fn create_mailbox(&self) -> Box<dyn Mailbox> {
        (self.mailbox_factory)()
    }

    pub fn with_mailbox<M>(mut self, factory: M) -> Self
    where
        M: Fn() -> Box<dyn Mailbox> + Send + Sync + 'static,
    {
        self.mailbox_factory = mailbox_factory_from(factory);
        self
    }
}

impl Clone for Props {
    fn clone(&self) -> Self {
        Self {
            factory: Arc::clone(&self.factory),
            strategy: self.strategy.clone(),
            mailbox_factory: Arc::clone(&self.mailbox_factory),
        }
    }
}

impl fmt::Debug for Props {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Props").finish_non_exhaustive()
    }
}
