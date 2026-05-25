use super::{
    actor::Actor,
    mailbox::{DefaultMailbox, Mailbox},
};
use std::{fmt, sync::Arc};

/// Actor factory used when spawning a new instance.
pub struct Props {
    factory: Arc<dyn Fn() -> Box<dyn Actor> + Send + Sync>,
}

impl Props {
    pub fn new<F, A>(factory: F) -> Self
    where
        F: Fn() -> A + Send + Sync + 'static,
        A: Actor,
    {
        Self {
            factory: Arc::new(move || Box::new(factory()) as Box<dyn Actor>),
        }
    }

    pub(crate) fn create(&self) -> Box<dyn Actor> {
        (self.factory)()
    }

    pub(crate) fn create_mailbox(&self) -> Box<dyn Mailbox> {
        Box::new(DefaultMailbox::default())
    }
}

impl Clone for Props {
    fn clone(&self) -> Self {
        Self {
            factory: Arc::clone(&self.factory),
        }
    }
}

impl fmt::Debug for Props {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Props").finish_non_exhaustive()
    }
}
