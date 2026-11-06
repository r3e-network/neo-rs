//! Adapter layer bridging the decomposed `neo-system` runtime to the canonical
//! ledger and P2P port contracts.
//!
//! This module holds the *migration seams* approved in task 102. Each adapter
//! implements a trait owned by a sibling crate (`neo_ledger::persistence`,
//! `neo_p2p::ports`) while naming only that crate's canonical vocabulary,
//! `neo_primitives`, `crate::runtime`, `std`, and `tokio` — never a `neo-core`
//! type, `DataCache`, or `StoreSnapshot` in a port parameter. The concrete wiring
//! to `neo-core` lives in the integration lane (task 89), the only place allowed
//! to touch both sides.
//!
//! # Contents
//!
//! * [`participant_bridge`] — [`HandlerParticipantBridge`], the temporary
//!   middleware adapting a committing/committed handler pair to the canonical
//!   [`neo_ledger::persistence::CommitParticipant`] lifecycle
//!   (`precommit`/`discard`/`committed`). Removed once every collaborator has a
//!   direct participant implementation.
//! * [`network_guard`] — [`NetworkGuard`], the L.4 constructor-injected network
//!   guard that replaces the per-block `Any` downcast in logs/tracker with a
//!   one-time startup comparison that refuses to start on mismatch.
//! * [`dispatch_adapter`] — [`InboundMessageDispatch`] and [`ActorInboundDispatch`],
//!   implementing [`neo_p2p::ports::InboundDispatchPort`] to map remote message
//!   handling onto the P2P inbound dispatch seam without exposing `ActorRef` or a
//!   system context.
//!
//! # Deliberately absent
//!
//! `wallet-changed` handling is deferred to P9 per decision L.5 and is **not**
//! implemented here.
//!
//! # Wiring status
//!
//! Dest-only: like `neo_p2p::ports` and `neo_ledger::persistence`, this module is
//! staged source. Task 89 declares `pub mod adapters;` from the `neo-system`
//! crate root, adds the `neo-ledger` / `neo-p2p` manifest dependencies, and hooks
//! the adapters into the participant registry and the P2P session port bundle.

pub mod dispatch_adapter;
pub mod network_guard;
pub mod participant_bridge;

pub use dispatch_adapter::{ActorInboundDispatch, InboundFrameDelivered, InboundMessageDispatch};
pub use network_guard::{NetworkGuard, NetworkMismatch};
pub use participant_bridge::HandlerParticipantBridge;
