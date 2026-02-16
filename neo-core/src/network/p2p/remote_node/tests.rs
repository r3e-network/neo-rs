use super::{
    HandshakeGateDecision, PendingKnownHashes, RemoteNode, UInt256, message_handlers,
    register_message_received_handler,
};
use crate::i_event_handlers::IMessageReceivedHandler;
use crate::network::p2p::{message::Message, message_command::MessageCommand, timeouts};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

fn make_hash(byte: u8) -> UInt256 {
    let mut data = [0u8; 32];
    data[0] = byte;
    UInt256::from(data)
}

#[test]
fn prune_older_than_removes_stale_entries() {
    let now = Instant::now();
    let mut cache = PendingKnownHashes::new(4);

    // Use timestamps within MAX_PENDING_TTL (60s) to avoid auto-prune during try_add.
    // Entry 1 at now - 55s, Entry 2 at now - 5s.
    // When entry 2 is added, auto-prune cutoff is (now - 5) - 60 = now - 65s,
    // which won't remove entry 1 (at now - 55s).
    cache.try_add(make_hash(1), now - Duration::from_secs(55));
    cache.try_add(make_hash(2), now - Duration::from_secs(5));

    // Prune entries older than now - 30s; should remove entry 1 but keep entry 2
    let removed = cache.prune_older_than(now - Duration::from_secs(30));
    assert_eq!(removed, 1);
    assert!(!cache.contains(&make_hash(1)));
    assert!(cache.contains(&make_hash(2)));
}

#[test]
fn timeout_stats_increment() {
    timeouts::reset();
    timeouts::inc_handshake_timeout();
    timeouts::inc_read_timeout();
    timeouts::inc_write_timeout();
    let stats = timeouts::stats();
    assert_eq!(stats.handshake, 1);
    assert_eq!(stats.read, 1);
    assert_eq!(stats.write, 1);
}

#[test]
fn timeout_stats_logged() {
    timeouts::reset();
    timeouts::log_stats();
}

#[derive(Default)]
struct TestHandler {
    invocations: AtomicUsize,
}

impl IMessageReceivedHandler for TestHandler {
    fn remote_node_message_received_handler(
        &self,
        _system: &dyn std::any::Any,
        _message: &Message,
    ) -> bool {
        self.invocations.fetch_add(1, Ordering::Relaxed);
        true
    }
}

#[test]
fn register_handler_tracks_subscription() {
    message_handlers::reset();

    let handler = Arc::new(TestHandler::default());
    let subscription = register_message_received_handler(handler.clone());
    let count = message_handlers::with_handlers(|handlers| handlers.len());
    assert_eq!(count, 1);

    drop(subscription);
    let count = message_handlers::with_handlers(|handlers| handlers.len());
    assert_eq!(count, 0);
}

#[test]
fn summarize_alert_payload_strips_control_chars() {
    let payload = b"Node alert:\nRestart\x07 now";
    let summary = RemoteNode::summarize_alert_payload(payload);
    assert_eq!(summary, "Node alert:\nRestart now");
}

#[test]
fn summarize_alert_payload_serializes_binary_as_hex() {
    let payload = [0xFFu8, 0x00, 0x34, 0xAB];
    let summary = RemoteNode::summarize_alert_payload(&payload);
    assert_eq!(summary, "0xff0034ab");
}

#[test]
fn summarize_alert_payload_truncates_output() {
    let payload = vec![b'a'; RemoteNode::MAX_ALERT_LOG_BYTES + 8];
    let summary = RemoteNode::summarize_alert_payload(&payload);
    assert!(summary.ends_with("..."));
    assert_eq!(
        summary.len(),
        RemoteNode::MAX_ALERT_LOG_BYTES + 3,
        "appends ellipsis when payload is longer than capture window"
    );
}

#[test]
fn handshake_gate_requires_version_first() {
    let version = RemoteNode::handshake_gate_decision(false, false, MessageCommand::Version);
    assert!(matches!(version, HandshakeGateDecision::AcceptVersion));

    let verack = RemoteNode::handshake_gate_decision(false, false, MessageCommand::Verack);
    assert!(matches!(verack, HandshakeGateDecision::Reject(_)));

    let ping = RemoteNode::handshake_gate_decision(false, false, MessageCommand::Ping);
    assert!(matches!(ping, HandshakeGateDecision::Reject(_)));
}

#[test]
fn handshake_gate_requires_verack_after_version() {
    let verack = RemoteNode::handshake_gate_decision(true, false, MessageCommand::Verack);
    assert!(matches!(verack, HandshakeGateDecision::AcceptVerack));

    let ping = RemoteNode::handshake_gate_decision(true, false, MessageCommand::Ping);
    assert!(matches!(ping, HandshakeGateDecision::Reject(_)));
}

#[test]
fn handshake_gate_rejects_duplicate_handshake_messages_after_completion() {
    let version = RemoteNode::handshake_gate_decision(true, true, MessageCommand::Version);
    assert!(matches!(version, HandshakeGateDecision::Reject(_)));

    let verack = RemoteNode::handshake_gate_decision(true, true, MessageCommand::Verack);
    assert!(matches!(verack, HandshakeGateDecision::Reject(_)));

    let ping = RemoteNode::handshake_gate_decision(true, true, MessageCommand::Ping);
    assert!(matches!(ping, HandshakeGateDecision::AcceptProtocol));
}
