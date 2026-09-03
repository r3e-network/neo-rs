//! Connection lifecycle helpers for `RemoteNode`.

use super::RemoteNode;
use crate::network::MessageCommand;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum HandshakeGateDecision {
    AcceptVersion,
    AcceptVerack,
    AcceptProtocol,
    Reject(&'static str),
}

impl RemoteNode {
    pub(super) fn handshake_gate_decision(
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
}
