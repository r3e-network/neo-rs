//! Lightweight actor runtime inspired by Akka.NET.
//!
//! Provides asynchronous actors, hierarchical supervision, message passing,
//! and basic scheduling utilities built on top of tokio channels.

mod actor;
mod actor_ref;
mod actor_system;
mod context;
mod error;
mod event_stream;
mod mailbox;
mod message;
mod props;
mod scheduler;

pub use actor::{Actor, ActorResult, SupervisorDirective};
pub use actor_ref::ActorRef;
pub use actor_system::{ActorPath, ActorSystem, ActorSystemHandle};
pub use context::ActorContext;
pub use error::{AkkaError, AkkaResult};
pub use event_stream::EventStreamHandle;
pub use mailbox::{
    default_mailbox_factory, priority_mailbox_factory, Cancelable, DefaultMailbox, Mailbox,
    MailboxFactory, PriorityMailbox, PriorityMailboxConfig,
};
pub use message::{Envelope, MailboxMessage, SystemMessage, Terminated};
pub use props::Props;
pub use scheduler::{ScheduleHandle, Scheduler};

#[cfg(test)]
mod tests;
