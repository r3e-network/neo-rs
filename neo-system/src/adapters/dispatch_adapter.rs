//! Inbound-message dispatch adapter implementing [`neo_p2p::ports::InboundDispatchPort`].
//!
//! The legacy network stack let a `RemoteNode` reach through
//! `system.neo_system()` and a global `message_handlers` registry to run every
//! registered `remote_node_message_received_handler` before its own protocol
//! processing. That coupled inbound-frame veto to `ActorRef` / a system context
//! and to `neo-core`.
//!
//! The decomposed P2P layer replaces that seam with the typed
//! [`InboundDispatchPort`], whose only job is: *offer one inbound frame to an
//! external handler and let it veto*. This module is the `neo-system` side of
//! that seam. It provides adapters that implement the port while keeping the
//! actor runtime private:
//!
//! * [`InboundMessageDispatch`] — wraps a synchronous veto handler and preserves
//!   the port contract (`true` continues default processing, `false` cancels).
//! * [`ActorInboundDispatch`] — forwards each frame to an observer actor. It uses
//!   [`neo_actors::ActorRef`] *internally* but never exposes it through the port
//!   signature.
//!
//! # Contract (preserved from `dispatch_message_received`)
//!
//! * Return `true` to let the session run its normal validated handling.
//! * Return `false` to cancel: the frame is dropped without further processing.
//!   If several handlers are registered, a single `false` cancels (short-circuit).
//! * **Fail-open-to-default**: when no external handler is registered — the
//!   original's "no system / empty handlers" case — the adapter returns `true`.
//!   The session's own path is the fully validated one; the hook exists only to
//!   let an embedding application veto or observe frames, never to inject
//!   unvalidated handling.
//!
//! # Type discipline
//!
//! The port surface names only `neo-p2p`'s own vocabulary ([`PeerHandle`],
//! [`MessageCommand`]) and `std`. No `ActorRef`, `ActorContext`, `SystemContext`,
//! or `neo-core` type appears in any port parameter. The wiring lane (task 89)
//! constructs an adapter from the concrete handler registry and hands it to the
//! session as `Arc<dyn InboundDispatchPort>`.

use crate::network::p2p::MessageCommand;
use crate::network::p2p::ports::{InboundDispatchPort, PeerHandle};
use neo_actors::ActorRef;
use std::fmt;

/// Boxed synchronous veto handler body.
type DispatchHandler = dyn Fn(PeerHandle, MessageCommand, &[u8]) -> bool + Send + Sync;

/// Closure-backed inbound dispatch adapter preserving the veto contract.
///
/// Wraps an optional synchronous handler. When a handler is present its return
/// value is the veto decision; when absent the adapter **fails open** and returns
/// `true`, matching the "no system / empty handlers" case of the original
/// `dispatch_message_received`.
///
/// # Example
///
/// ```rust,ignore
/// // Veto every `Inv` frame; let all other frames fall through to default handling.
/// let dispatch = InboundMessageDispatch::new(|_peer, command, _payload| {
///     command != MessageCommand::Inv
/// });
/// let ports = RemoteSessionPorts { dispatch: Arc::new(dispatch), /* .. */ };
/// ```
pub struct InboundMessageDispatch {
    handler: Option<Box<DispatchHandler>>,
}

impl InboundMessageDispatch {
    /// Wraps a synchronous veto handler.
    ///
    /// The handler receives the opaque peer, the decoded command, and the raw
    /// payload bytes, and returns `true` to continue default processing or
    /// `false` to cancel it.
    pub fn new<H>(handler: H) -> Self
    where
        H: Fn(PeerHandle, MessageCommand, &[u8]) -> bool + Send + Sync + 'static,
    {
        Self {
            handler: Some(Box::new(handler)),
        }
    }

    /// Builds an adapter with no handler that always fails open (returns `true`).
    ///
    /// This is the direct replacement for the legacy "no system / empty handler
    /// registry" path.
    pub fn passthrough() -> Self {
        Self { handler: None }
    }

    /// Returns `true` when a veto handler is attached.
    pub fn has_handler(&self) -> bool {
        self.handler.is_some()
    }
}

impl Default for InboundMessageDispatch {
    fn default() -> Self {
        Self::passthrough()
    }
}

impl InboundDispatchPort for InboundMessageDispatch {
    fn dispatch_inbound(&self, peer: PeerHandle, command: MessageCommand, payload: &[u8]) -> bool {
        match &self.handler {
            Some(handler) => handler(peer, command, payload),
            // Fail-open-to-default: no external handler means the session runs
            // its own fully validated path.
            None => true,
        }
    }
}

impl fmt::Debug for InboundMessageDispatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("InboundMessageDispatch")
            .field("handler", &self.handler.as_ref().map(|_| "<veto handler>"))
            .finish()
    }
}

/// Message forwarded to an observer actor by [`ActorInboundDispatch`].
///
/// A plain, owned value type (`Send + 'static`) so it can be delivered through
/// [`ActorRef::tell`]. The receiving actor downcasts it by concrete type; the
/// port itself still never exposes the actor reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InboundFrameDelivered {
    /// The session that received the frame.
    pub peer: PeerHandle,
    /// The decoded protocol message command.
    pub command: MessageCommand,
    /// Raw payload bytes.
    pub payload: Vec<u8>,
}

impl InboundFrameDelivered {
    /// Builds a delivery notice from a peer, command, and borrowed payload.
    pub fn new(peer: PeerHandle, command: MessageCommand, payload: &[u8]) -> Self {
        Self {
            peer,
            command,
            payload: payload.to_vec(),
        }
    }
}

/// Actor-forwarding dispatch adapter for observation-only hooks.
///
/// Holds an [`ActorRef`] internally and `tell`s it an [`InboundFrameDelivered`]
/// for every inbound frame. Because an actor round-trip cannot produce a
/// synchronous veto decision, this adapter always returns `true`
/// (fail-open-to-default): it lets an embedding application *observe* frames
/// without ever injecting unvalidated handling. Use [`InboundMessageDispatch`]
/// when a real veto is required.
///
/// The [`ActorRef`] lives only inside this struct; the [`InboundDispatchPort`]
/// signature it implements names just [`PeerHandle`], [`MessageCommand`], and the
/// payload bytes.
#[derive(Debug, Clone)]
pub struct ActorInboundDispatch {
    observer: ActorRef,
}

impl ActorInboundDispatch {
    /// Wraps an observer actor that receives every inbound frame.
    pub fn new(observer: ActorRef) -> Self {
        Self { observer }
    }

    /// Returns a reference to the observer actor.
    ///
    /// Exposed for the wiring lane that owns the actor registry; it is not part
    /// of the dispatch port surface.
    pub fn observer(&self) -> &ActorRef {
        &self.observer
    }
}

impl InboundDispatchPort for ActorInboundDispatch {
    fn dispatch_inbound(&self, peer: PeerHandle, command: MessageCommand, payload: &[u8]) -> bool {
        // Fire-and-forget observation. A delivery failure (full or stopped
        // mailbox) must not cancel the session's own validated handling, so the
        // result is intentionally ignored and we fail open.
        let _ = self
            .observer
            .tell(InboundFrameDelivered::new(peer, command, payload));
        true
    }
}

// Both adapters hold only `Send + Sync + 'static` state, so they satisfy the
// `InboundDispatchPort: Send + Sync` supertraits and can be shared as
// `Arc<dyn InboundDispatchPort>` across session tasks.
const _: fn() = || {
    fn assert_port<T: InboundDispatchPort + Send + Sync + 'static>() {}
    assert_port::<InboundMessageDispatch>();
    assert_port::<ActorInboundDispatch>();
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_fails_open() {
        let dispatch = InboundMessageDispatch::passthrough();
        assert!(!dispatch.has_handler());
        assert!(dispatch.dispatch_inbound(PeerHandle::new(1), MessageCommand::Ping, &[]));
    }

    #[test]
    fn default_is_passthrough() {
        let dispatch = InboundMessageDispatch::default();
        assert!(!dispatch.has_handler());
        assert!(dispatch.dispatch_inbound(PeerHandle::new(1), MessageCommand::Block, &[0xAA]));
    }

    #[test]
    fn handler_veto_is_honoured() {
        let dispatch =
            InboundMessageDispatch::new(|_peer, command, _payload| command != MessageCommand::Inv);
        assert!(dispatch.has_handler());

        assert!(dispatch.dispatch_inbound(PeerHandle::new(1), MessageCommand::Ping, &[]));
        assert!(!dispatch.dispatch_inbound(PeerHandle::new(1), MessageCommand::Inv, &[1, 2]));
    }

    #[test]
    fn handler_observes_peer_and_payload() {
        let dispatch = InboundMessageDispatch::new(|peer, _command, payload| {
            peer.as_u64() == 7 && payload == [0xDE, 0xAD]
        });

        assert!(dispatch.dispatch_inbound(
            PeerHandle::new(7),
            MessageCommand::Transaction,
            &[0xDE, 0xAD]
        ));
        assert!(!dispatch.dispatch_inbound(
            PeerHandle::new(8),
            MessageCommand::Transaction,
            &[0xDE, 0xAD]
        ));
    }

    #[test]
    fn delivered_notice_owns_the_payload() {
        let notice = InboundFrameDelivered::new(PeerHandle::new(3), MessageCommand::Block, &[9, 9]);
        assert_eq!(notice.peer, PeerHandle::new(3));
        assert_eq!(notice.command, MessageCommand::Block);
        assert_eq!(notice.payload, vec![9, 9]);
    }
}
