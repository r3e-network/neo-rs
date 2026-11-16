//! Handshake gating for inbound peer messages.
//!
//! Mirrors `neo-core::network::p2p::remote_node::lifecycle`'s
//! `HandshakeGateDecision`. Before a session runs its normal protocol handling
//! it must decide whether an inbound command is legal for the current handshake
//! phase: version-first, then verack, then everything else. This module keeps
//! that decision as a pure function so it can be unit-tested without a socket.

use super::message_command::MessageCommand;

/// Outcome of the handshake gate for one inbound command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandshakeGateDecision {
    /// The command is the expected `Version`; accept and record it.
    AcceptVersion,
    /// The command is the expected `Verack`; accept and complete the handshake.
    AcceptVerack,
    /// The handshake is complete; accept the command as normal protocol traffic.
    AcceptProtocol,
    /// The command is illegal for the current phase; reject with a static reason.
    Reject(&'static str),
}

/// Decides how a session should treat `command` given its handshake progress.
///
/// * Before any `Version` is received only `Version` is legal.
/// * After `Version` but before the handshake completes only `Verack` is legal.
/// * Once complete, `Version`/`Verack` are rejected as duplicates and every
///   other command is accepted as protocol traffic.
pub fn handshake_gate_decision(
    version_received: bool,
    handshake_complete: bool,
    command: MessageCommand,
) -> HandshakeGateDecision {
    if !version_received {
        return match command {
            MessageCommand::Version => HandshakeGateDecision::AcceptVersion,
            _ => HandshakeGateDecision::Reject("expected version message before handshake"),
        };
    }

    if !handshake_complete {
        return match command {
            MessageCommand::Verack => HandshakeGateDecision::AcceptVerack,
            _ => HandshakeGateDecision::Reject("expected verack message after version"),
        };
    }

    match command {
        MessageCommand::Version | MessageCommand::Verack => {
            HandshakeGateDecision::Reject("duplicate handshake message after completion")
        }
        _ => HandshakeGateDecision::AcceptProtocol,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_required_first() {
        assert_eq!(
            handshake_gate_decision(false, false, MessageCommand::Version),
            HandshakeGateDecision::AcceptVersion
        );
        assert_eq!(
            handshake_gate_decision(false, false, MessageCommand::Ping),
            HandshakeGateDecision::Reject("expected version message before handshake")
        );
    }

    #[test]
    fn verack_is_required_after_version() {
        assert_eq!(
            handshake_gate_decision(true, false, MessageCommand::Verack),
            HandshakeGateDecision::AcceptVerack
        );
        assert_eq!(
            handshake_gate_decision(true, false, MessageCommand::Ping),
            HandshakeGateDecision::Reject("expected verack message after version")
        );
    }

    #[test]
    fn protocol_commands_are_accepted_after_completion() {
        assert_eq!(
            handshake_gate_decision(true, true, MessageCommand::Ping),
            HandshakeGateDecision::AcceptProtocol
        );
        assert_eq!(
            handshake_gate_decision(true, true, MessageCommand::Inv),
            HandshakeGateDecision::AcceptProtocol
        );
    }

    #[test]
    fn duplicate_handshake_messages_are_rejected_after_completion() {
        assert_eq!(
            handshake_gate_decision(true, true, MessageCommand::Version),
            HandshakeGateDecision::Reject("duplicate handshake message after completion")
        );
        assert_eq!(
            handshake_gate_decision(true, true, MessageCommand::Verack),
            HandshakeGateDecision::Reject("duplicate handshake message after completion")
        );
    }
}
