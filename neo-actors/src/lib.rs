//! Lightweight actor runtime for async node components.
//!
//! Provides asynchronous actors, hierarchical supervision, message passing,
//! and basic scheduling utilities built on top of tokio channels.

mod actor;
mod actor_path;
mod actor_ref;
mod actor_system;
mod context;
mod error;
mod event_stream;
mod mailbox;
mod message;
mod props;
mod scheduler;
mod task_executor;

pub use actor::{Actor, ActorResult, SupervisorDirective};
pub use actor_path::ActorPath;
pub use actor_ref::ActorRef;
pub use actor_system::{ActorSystem, ActorSystemHandle};
#[doc(hidden)]
pub use actor_system::MailboxCommand;
pub use context::ActorContext;
pub use error::{ActorRuntimeError, ActorRuntimeResult};
pub use event_stream::EventStreamHandle;
pub use message::{Envelope, MailboxMessage, SystemMessage, Terminated};
pub use props::Props;
pub use scheduler::{Cancelable, ScheduleHandle, Scheduler};
pub use task_executor::{CancellationToken, TaskExecutor};

#[cfg(test)]
mod tests;
