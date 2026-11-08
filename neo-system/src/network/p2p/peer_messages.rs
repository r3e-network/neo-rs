//! Lightweight peer protocol handlers (ping/pong/addr/get-headers/headers).
//!
//! Mirrors `neo-core::network::p2p::remote_node::peer_messages`. The original was
//! a set of `impl RemoteNode` methods reaching through `NeoSystemContext`
//! (`self.system.*`), the actor `ctx.self_ref()` / `ctx.parent()`, and an async
//! `self.enqueue_message(..)`. Here every collaborator is a typed port from
//! [`crate::ports`], the peer is an opaque [`PeerHandle`], and handlers are pure
//! synchronous functions.
//!
//! Two adaptations depart from the actor original because the port surface is
//! deliberately smaller:
//!
//! * **`on_get_headers`** — the original read a header cache through
//!   `self.system.headers_from_index(..)`. [`LedgerQueryPort`] exposes no
//!   header-cache read, so headers are reconstructed from persisted blocks
//!   (`block_hash_at` → `try_get_block` → `block.header`), stopping at the first
//!   gap. This serves headers only up to the persisted tip; a headers-only cache
//!   ahead of persistence is a task-89 adapter concern, not this module's.
//! * **`on_addr`** — the original forwarded `PeerCommand::AddPeers { endpoints }`
//!   to the parent `LocalNode` actor. There is no parent handle and
//!   [`PeerCommandPort`] has no peer-discovery command, so `on_addr` *returns*
//!   the collected reachable endpoints for the session/transport owner to feed
//!   into its own connector. The `GetAddr` correlation flag is still consumed so
//!   an unsolicited `Addr` is ignored exactly as before.

use super::message_command::MessageCommand;
use super::payloads::addr_payload::AddrPayload;
use super::payloads::get_block_by_index_payload::GetBlockByIndexPayload;
use super::payloads::headers_payload::{HeadersPayload, MAX_HEADERS_COUNT};
use super::payloads::network_address_with_time::NetworkAddressWithTime;
use super::payloads::ping_payload::PingPayload;
use super::ports::{PeerHandle, RemoteSessionPorts};
use super::protocol_message::{NetworkMessage, ProtocolMessage};
use super::session_state::SessionState;
use neo_protocol::payloads::Header;
use std::collections::HashSet;
use std::net::SocketAddr;

/// Normalizes a signed protocol request count into a bounded `usize`.
///
/// A negative count is the protocol's "give me the maximum" sentinel; a
/// non-negative count is clamped to `max`. Mirrors the free function of the same
/// name in `neo-core`, and reused by [`crate::inventory`] for `GetBlocks` /
/// `GetBlockByIndex`.
pub fn normalize_request(count: i16, max: usize) -> usize {
    if count < 0 {
        max
    } else {
        (count as usize).min(max)
    }
}

/// Collects distinct, reachable TCP endpoints from an `Addr` payload.
///
/// Drops entries with no TCP/WS endpoint or a zero port and de-duplicates while
/// preserving first-seen order.
fn collect_reachable_endpoints(addresses: Vec<NetworkAddressWithTime>) -> Vec<SocketAddr> {
    let mut endpoints = Vec::with_capacity(addresses.len());
    let mut seen = HashSet::new();

    for address in addresses {
        if let Some(endpoint) = address.endpoint() {
            if endpoint.port() > 0 && seen.insert(endpoint) {
                endpoints.push(endpoint);
            }
        }
    }

    endpoints
}

/// Reconstructs up to `count` headers starting at `start_index` from persisted
/// blocks, stopping at the first height that is not persisted.
fn headers_from_index(ports: &RemoteSessionPorts, start_index: u32, count: usize) -> Vec<Header> {
    let mut headers = Vec::with_capacity(count);
    for offset in 0..count {
        let index = start_index.saturating_add(offset as u32);
        let Some(hash) = ports.ledger_query.block_hash_at(index) else {
            break;
        };
        let Some(block) = ports.ledger_query.try_get_block(&hash) else {
            break;
        };
        headers.push(block.header.clone());
    }
    headers
}

/// Handles an inbound `Ping`: records the peer height, forwards it to the task
/// manager, and replies with a `Pong` carrying the local height and same nonce.
pub fn on_ping(
    state: &mut SessionState,
    ports: &RemoteSessionPorts,
    peer: PeerHandle,
    payload: &PingPayload,
) -> Vec<NetworkMessage> {
    state.last_block_index = payload.last_block_index;
    let _ = ports
        .task_manager
        .update_peer(peer, payload.last_block_index);

    let local_index = ports.ledger_query.current_index();
    let pong = ProtocolMessage::pong_with_block_index(local_index, payload.nonce);
    vec![NetworkMessage::new(pong)]
}

/// Handles an inbound `Pong`: advances the peer height (never lowers it) and
/// forwards the report to the task manager so a peer that never sends `Ping`
/// still drives the sync pipeline.
pub fn on_pong(
    state: &mut SessionState,
    ports: &RemoteSessionPorts,
    peer: PeerHandle,
    payload: &PingPayload,
) {
    if payload.last_block_index > state.last_block_index {
        state.last_block_index = payload.last_block_index;
    }
    let _ = ports
        .task_manager
        .update_peer(peer, payload.last_block_index);
}

/// Handles an inbound `Addr` reply, returning the reachable endpoints to add.
///
/// An unsolicited `Addr` (no outstanding `GetAddr`) is ignored, matching the
/// actor original's `consume_sent_command` guard. The returned endpoints are the
/// caller's to feed into its peer connector; there is no discovery port to
/// forward them through.
pub fn on_addr(state: &mut SessionState, payload: AddrPayload) -> Vec<SocketAddr> {
    if !state.consume_sent_command(MessageCommand::GetAddr) {
        return Vec::new();
    }
    collect_reachable_endpoints(payload.address_list)
}

/// Handles an inbound `GetHeaders` request, replying with a `Headers` batch.
pub fn on_get_headers(
    ports: &RemoteSessionPorts,
    payload: &GetBlockByIndexPayload,
) -> Vec<NetworkMessage> {
    let count = normalize_request(payload.count, MAX_HEADERS_COUNT);
    if count == 0 {
        return Vec::new();
    }

    let headers = headers_from_index(ports, payload.index_start, count);
    if headers.is_empty() {
        return Vec::new();
    }

    vec![NetworkMessage::new(ProtocolMessage::Headers(
        HeadersPayload::create(headers),
    ))]
}

/// Handles an inbound `Headers` batch: forwards the tip as a peer-height hint,
/// submits the headers to the chain sink, and notifies the task manager.
pub fn on_headers(
    ports: &RemoteSessionPorts,
    peer: PeerHandle,
    payload: HeadersPayload,
) {
    if payload.headers.is_empty() {
        return;
    }

    // Forward the last delivered header index as a peer-height hint. The session
    // keeps the maximum of all reports, so this cannot lower a Version/Ping
    // height.
    if let Some(last) = payload.headers.last() {
        let _ = ports.task_manager.update_peer(peer, last.index());
    }

    let _ = ports.chain_sink.submit_headers(payload.headers, peer);
    let _ = ports.task_manager.headers(peer);
}

#[cfg(test)]
mod tests {
    use super::{collect_reachable_endpoints, normalize_request};
    use crate::payloads::node_capability::NodeCapability;
    use crate::payloads::network_address_with_time::NetworkAddressWithTime;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn address(ip: [u8; 4], port: u16) -> NetworkAddressWithTime {
        NetworkAddressWithTime::new(
            0,
            IpAddr::V4(Ipv4Addr::from(ip)),
            vec![NodeCapability::TcpServer { port }],
        )
    }

    #[test]
    fn addr_endpoint_collection_drops_zero_port_and_deduplicates() {
        let endpoints = collect_reachable_endpoints(vec![
            address([10, 0, 0, 1], 20333),
            address([10, 0, 0, 2], 0),
            address([10, 0, 0, 1], 20333),
            address([10, 0, 0, 3], 20334),
        ]);

        assert_eq!(
            endpoints,
            vec![
                SocketAddr::from(([10, 0, 0, 1], 20333)),
                SocketAddr::from(([10, 0, 0, 3], 20334)),
            ]
        );
    }

    #[test]
    fn negative_header_request_count_uses_protocol_max() {
        assert_eq!(normalize_request(-1, 2_000), 2_000);
    }

    #[test]
    fn header_request_count_is_capped_to_protocol_max() {
        assert_eq!(normalize_request(2_500, 2_000), 2_000);
        assert_eq!(normalize_request(512, 2_000), 512);
        assert_eq!(normalize_request(0, 2_000), 0);
    }
}
