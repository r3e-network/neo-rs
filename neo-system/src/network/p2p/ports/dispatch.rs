//! Inbound-message dispatch hook port (external handler veto).
//!
//! Replaces `RemoteNode::dispatch_message_received`, which reached through
//! `self.system.neo_system()` and the global `message_handlers` registry to run
//! every registered `remote_node_message_received_handler` before the session's
//! own protocol processing. The port keeps the same veto contract while naming
//! only the opaque peer handle and p2p's own `MessageCommand`.

use super::PeerHandle;
use crate::network::p2p::MessageCommand;

/// Offers an inbound frame to external handlers before default processing.
///
/// The session calls this at the top of its inbound path and only continues its
/// own protocol handling when the port returns `true`.
///
/// # Contract (preserved from `dispatch_message_received`)
///
/// * Return `true` to let the session run its normal validated handling.
/// * Return `false` to cancel: the frame is dropped without further processing.
///   If several handlers are registered, a single `false` cancels (short-circuit),
///   matching the original loop.
///
/// # Fail-open-to-default note
///
/// This hook deliberately fails *toward* default processing: when no external
/// handler is registered — the original's "no system / empty handlers" case —
/// the adapter must return `true`. That is safe because the session's own path
/// is the fully validated one; the hook exists only to let an embedding
/// application veto or observe frames, never to inject unvalidated handling.
pub trait InboundDispatchPort: Send + Sync {
    /// Offers one inbound frame to external handlers.
    ///
    /// * `peer` — the session that received the frame.
    /// * `command` — the decoded protocol message command.
    /// * `payload` — the raw payload bytes for handlers that need the wire body.
    ///
    /// Returns `true` to continue default processing, `false` to cancel it.
    fn dispatch_inbound(&self, peer: PeerHandle, command: MessageCommand, payload: &[u8]) -> bool;
}
