//! Generic asynchronous actor runtime extracted from `neo-core`.
//!
//! This crate is protocol-agnostic: message payloads travel as
//! `Box<dyn Any + Send>`, so it has **no** dependency on any other `neo-*`
//! crate. Domain actors (blockchain, consensus, task manager, ...) live in
//! their own crates and plug in through [`Props`].
//!
//! Provides asynchronous actors, hierarchical supervision, message passing,
//! and basic scheduling utilities built on top of tokio channels.
//!
//! # Core types
//!
//! - [`Actor`] — the trait every actor implements (`pre_start` / `handle` /
//!   `post_stop` / `on_failure`).
//! - [`ActorSystem`] — creates and supervises actors; owns the registry,
//!   scheduler and event stream.
//! - [`ActorRef`] — cheap, clonable, addressable handle for `tell` / `watch`.
//! - [`ActorContext`] — per-invocation view of self, sender, parent, children.
//! - [`Props`] — actor factory, also used to restart an actor after failure.
//! - [`Scheduler`] / [`ScheduleHandle`] — delayed and recurring messages.
//! - [`EventStreamHandle`] — type-keyed publish/subscribe over actors.
//!
//! # Example
//!
//! ```rust,ignore
//! let system = ActorSystem::new("my-node")?;
//! let worker = system.actor_of(Props::new(WorkerActor::default), "worker")?;
//! worker.tell(DoWork { id: 1 })?;
//! system.shutdown().await?;
//! ```

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
pub use error::{ActorRuntimeError, ActorRuntimeResult};
pub use event_stream::EventStreamHandle;
pub use message::{Envelope, MailboxMessage, SystemMessage, Terminated};
pub use props::Props;
pub use scheduler::{Cancelable, ScheduleHandle, Scheduler};

#[cfg(test)]
mod tests;
