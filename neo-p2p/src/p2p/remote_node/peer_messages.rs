//! Lightweight peer protocol handlers for `RemoteNode`.

use super::RemoteNode;
use crate::p2p::MessageCommand;
use crate::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::p2p::payloads::{
    addr_payload::AddrPayload,
    get_block_by_index_payload::GetBlockByIndexPayload,
    headers_payload::{HeadersPayload, MAX_HEADERS_COUNT},
    network_address_with_time::NetworkAddressWithTime,
    ping_payload::PingPayload,
};
use crate::p2p::peer::{PeerCommand, is_intranet_address};
use neo_actors::{ActorContext, ActorResult};
use neo_ledger::BlockchainCommand;
use std::collections::HashSet;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::warn;

impl RemoteNode {
    pub(super) async fn on_ping(
        &mut self,
        payload: &PingPayload,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        self.last_block_index = payload.last_block_index;
        if let Err(err) = self
            .system
            .task_manager()
            .update_peer(ctx.self_ref(), payload.last_block_index)
        {
            warn!(target: "neo", error = %err, "failed to forward peer height update to task manager");
        }
        let local_index = self.current_local_block_index();
        let pong = ProtocolMessage::pong_with_block_index(local_index, payload.nonce);
        self.enqueue_message(NetworkMessage::new(pong)).await
    }

    pub(super) fn on_pong(&mut self, payload: &PingPayload, ctx: &ActorContext) {
        if payload.last_block_index > self.last_block_index {
            self.last_block_index = payload.last_block_index;
        }
        // C# forwards Pong heights to TaskManager; without this a peer that
        // never sends PING stays frozen at its VERSION height and the sync
        // pipeline stops issuing requests.
        if let Err(err) = self
            .system
            .task_manager()
            .update_peer(ctx.self_ref(), payload.last_block_index)
        {
            warn!(target: "neo", error = %err, "failed to forward peer height update from pong");
        }
    }

    pub(super) fn on_addr(&mut self, payload: AddrPayload, ctx: &ActorContext) {
        if !self.consume_sent_command(MessageCommand::GetAddr) {
            return;
        }

        let endpoints = collect_reachable_endpoints(payload.address_list);
        if endpoints.is_empty() {
            return;
        }

        if let Some(parent) = ctx.parent()
            && let Err(err) = parent.tell(PeerCommand::AddPeers { endpoints })
        {
            warn!(
                target: "neo",
                error = %err,
                "failed to forward peer addresses to local node"
            );
        }
    }

    pub(super) async fn on_get_headers(&mut self, payload: GetBlockByIndexPayload) -> ActorResult {
        let count = Self::normalize_request(payload.count, MAX_HEADERS_COUNT);
        let headers = self.system.headers_from_index(payload.index_start, count);

        if headers.is_empty() {
            return Ok(());
        }

        let message =
            NetworkMessage::new(ProtocolMessage::Headers(HeadersPayload::create(headers)));
        self.enqueue_message(message).await
    }

    pub(super) async fn on_headers(&mut self, payload: HeadersPayload, ctx: &ActorContext) {
        // Forward the last delivered header index as a peer-height hint (C#
        // updates TaskManager from OnHeaders). The session keeps the maximum
        // of all reports, so this cannot lower a Version/Ping height.
        if !payload.headers.is_empty() {
            if let Some(last) = payload.headers.last()
                && let Err(err) = self
                    .system
                    .task_manager()
                    .update_peer(ctx.self_ref(), last.index())
            {
                warn!(target: "neo", error = %err, "failed to forward header height to task manager");
            }
            if let Err(err) = self
                .system
                .blockchain()
                .tell_from_async(
                    BlockchainCommand::Headers(
                        payload
                            .headers
                            .into_iter()
                            .map(|h| {
                                let block_header: neo_protocol::execution::BlockHeader = h.into();
                                Arc::new(neo_protocol::execution::Block::new(
                                    block_header,
                                    Vec::new(),
                                ))
                            })
                            .collect(),
                    ),
                    Some(ctx.self_ref()),
                )
                .await
            {
                warn!(target: "neo", error = %err, "failed to forward headers to blockchain");
            }

            if let Err(err) = self.system.task_manager().headers(ctx.self_ref()) {
                warn!(target: "neo", error = %err, "failed to notify task manager about headers");
            }
        }
    }

    fn consume_sent_command(&mut self, command: MessageCommand) -> bool {
        self.sent_commands.take(command)
    }

    pub(super) fn normalize_request(count: i16, max: usize) -> usize {
        normalize_request(count, max)
    }

    pub(super) fn current_local_block_index(&self) -> u32 {
        self.system.current_block_index()
    }
}

fn normalize_request(count: i16, max: usize) -> usize {
    if count < 0 {
        max
    } else {
        (count as usize).min(max)
    }
}

fn collect_reachable_endpoints(addresses: Vec<NetworkAddressWithTime>) -> Vec<SocketAddr> {
    let mut endpoints = Vec::with_capacity(addresses.len());
    let mut seen = HashSet::new();

    for address in addresses {
        if let Some(endpoint) = address.endpoint()
            && endpoint.port() > 0
            // C# v3.11.2 `Peer.OnReceive` (Addr case): intranet endpoints must
            // not be turned into outbound targets, so they are dropped here.
            // (The C# filter additionally excludes the local node's own
            // addresses, which are handled at connection time.)
            && !is_intranet_address(&endpoint.ip())
            && seen.insert(endpoint)
        {
            endpoints.push(endpoint);
        }
    }

    endpoints
}

#[cfg(test)]
mod tests {
    use super::{collect_reachable_endpoints, normalize_request};
    use crate::p2p::payloads::network_address_with_time::NetworkAddressWithTime;
    use crate::p2p::payloads::node_capability::tcp_server;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    fn address(ip: [u8; 4], port: u16) -> NetworkAddressWithTime {
        NetworkAddressWithTime::new(0, IpAddr::V4(Ipv4Addr::from(ip)), vec![tcp_server(port)])
    }

    fn address6(ip: Ipv6Addr, port: u16) -> NetworkAddressWithTime {
        NetworkAddressWithTime::new(0, IpAddr::V6(ip), vec![tcp_server(port)])
    }

    #[test]
    fn addr_endpoint_collection_drops_zero_port_and_deduplicates() {
        let endpoints = collect_reachable_endpoints(vec![
            address([51, 0, 0, 1], 20333),
            address([51, 0, 0, 2], 0),
            address([51, 0, 0, 1], 20333),
            address([51, 0, 0, 3], 20334),
        ]);

        assert_eq!(
            endpoints,
            vec![
                SocketAddr::from(([51, 0, 0, 1], 20333)),
                SocketAddr::from(([51, 0, 0, 3], 20334)),
            ]
        );
    }

    #[test]
    fn addr_endpoint_collection_drops_intranet_addresses() {
        let endpoints = collect_reachable_endpoints(vec![
            address([10, 0, 0, 1], 20333),     // 10/8
            address([0, 0, 0, 0], 20333),      // 0.0.0.0/8
            address([100, 64, 0, 1], 20333),   // 100.64/10 CGNAT
            address([100, 128, 0, 1], 20333),  // outside CGNAT: kept
            address([127, 0, 0, 1], 20333),    // 127/8 loopback
            address([169, 254, 1, 1], 20333),  // 169.254/16 link-local
            address([172, 15, 255, 1], 20333), // below 172.16/12: kept
            address([172, 16, 0, 1], 20333),   // 172.16/12
            address([172, 31, 255, 1], 20333), // 172.31/16: still 172.16/12
            address([172, 32, 0, 1], 20333),   // above 172.16/12: kept
            address([192, 168, 1, 1], 20333),  // 192.168/16
            address([8, 8, 8, 8], 20333),      // public: kept
        ]);

        assert_eq!(
            endpoints,
            vec![
                SocketAddr::from(([100, 128, 0, 1], 20333)),
                SocketAddr::from(([172, 15, 255, 1], 20333)),
                SocketAddr::from(([172, 32, 0, 1], 20333)),
                SocketAddr::from(([8, 8, 8, 8], 20333)),
            ]
        );
    }

    #[test]
    fn addr_endpoint_collection_applies_ipv6_intranet_rules() {
        let endpoints = collect_reachable_endpoints(vec![
            address6(Ipv6Addr::LOCALHOST, 20333),   // ::1
            address6(Ipv6Addr::UNSPECIFIED, 20333), // ::
            address6(Ipv6Addr::new(0xfe80, 0, 0, 0, 0, 0, 0, 1), 20333), // link-local
            address6(Ipv6Addr::new(0xfec0, 0, 0, 0, 0, 0, 0, 1), 20333), // site-local
            address6(Ipv6Addr::new(0xfc00, 0, 0, 0, 0, 0, 0, 1), 20333), // fc00::/7
            address6(Ipv6Addr::new(0xfd12, 0, 0, 0, 0, 0, 0, 1), 20333), // fd00::/8
            address6(Ipv6Addr::new(0xfe10, 0, 0, 0, 0, 0, 0, 1), 20333), // kept
            address6(Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1), 20333), // kept
            address6(Ipv6Addr::new(0, 0, 0, 0, 0, 0xffff, 0x0a00, 0x0804), 20333), // ::ffff:10.0.8.4
        ]);

        assert_eq!(
            endpoints,
            vec![
                SocketAddr::from((Ipv6Addr::new(0xfe10, 0, 0, 0, 0, 0, 0, 1), 20333)),
                SocketAddr::from((Ipv6Addr::new(0x2001, 0xdb8, 0, 0, 0, 0, 0, 1), 20333)),
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
