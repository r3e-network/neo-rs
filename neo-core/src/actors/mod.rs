//! Lightweight actor runtime inspired by Akka.NET.
//!
//! This module provides the primitives required by the neo-rs project to model
//! the Akka-based architecture used in the C# implementation.  It offers
//! asynchronous actors, hierarchical supervision, message passing and basic
//! scheduling utilities built on top of **ractor**.
//!
//! ## Architecture
//!
//! This module maintains the same public API as before but uses ractor internally
//! for actor spawning and message routing. The `Actor` trait signature remains
//! unchanged (`&mut self` in handle), with interior mutability bridging to ractor's
//! `&self` model.
//!
//! Previously in separate neo-akka crate, now inlined into neo-core with ractor backend.

mod actor;
mod actor_ref;
mod actor_system;
mod context;
mod error;
mod event_stream;
mod inbox;
mod mailbox;
mod message;
mod props;
mod ractor_bridge;
mod scheduler;
mod supervision;

pub use actor::{Actor, ActorResult};
pub use actor_ref::ActorRef;
pub use actor_system::{ActorPath, ActorSystem, ActorSystemHandle};
pub use context::ActorContext;
pub use error::{AkkaError, AkkaResult};
pub use event_stream::EventStreamHandle;
pub use inbox::Inbox;
pub use mailbox::{
    Cancelable, DefaultMailbox, Mailbox, MailboxFactory, PriorityMailbox, PriorityMailboxConfig,
    default_mailbox_factory, priority_mailbox_factory,
};
pub use message::{Envelope, Idle, MailboxMessage, SystemMessage, Terminated};
pub use props::Props;
pub use scheduler::{ScheduleHandle, Scheduler};
pub use supervision::{SupervisorDirective, SupervisorStrategy};

#[cfg(test)]
mod tests;
