//! Lightweight peer protocol handlers for `RemoteNode`.

use super::RemoteNode;
use crate::ledger::blockchain::BlockchainCommand;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::network::p2p::payloads::{
    addr_payload::AddrPayload,
    get_block_by_index_payload::GetBlockByIndexPayload,
    headers_payload::{HeadersPayload, MAX_HEADERS_COUNT},
    network_address_with_time::NetworkAddressWithTime,
    ping_payload::PingPayload,
};
use crate::network::p2p::peer::PeerCommand;
use crate::network::p2p::task_manager::TaskManagerCommand;
use crate::network::MessageCommand;
use crate::runtime::{ActorContext, ActorResult};
use std::collections::HashSet;
use std::net::SocketAddr;
use tracing::warn;

impl RemoteNode {
    pub(super) async fn on_ping(
        &mut self,
        payload: &PingPayload,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        self.last_block_index = payload.last_block_index;
        if let Err(err) = self.system.task_manager.tell_from(
            TaskManagerCommand::Update {
                last_block_index: payload.last_block_index,
            },
            Some(ctx.self_ref()),
        ) {
            warn!(target: "neo", error = %err, "failed to forward peer height update to task manager");
        }
        let local_index = self.current_local_block_index();
        let pong = ProtocolMessage::pong_with_block_index(local_index, payload.nonce);
        self.enqueue_message(NetworkMessage::new(pong)).await
    }

    pub(super) fn on_pong(&mut self, payload: &PingPayload) {
        if payload.last_block_index > self.last_block_index {
            self.last_block_index = payload.last_block_index;
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

        if let Some(parent) = ctx.parent() {
            if let Err(err) = parent.tell(PeerCommand::AddPeers { endpoints }) {
                warn!(
                    target: "neo",
                    error = %err,
                    "failed to forward peer addresses to local node"
                );
            }
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
        // The peer height comes from Version/Ping. Updating it from headers
        // would cap the session at the header frontier.
        if !payload.headers.is_empty() {
            let headers = payload.headers.clone();
            if let Err(err) = self
                .system
                .blockchain
                .tell_from_async(
                    BlockchainCommand::Headers(headers.clone()),
                    Some(ctx.self_ref()),
                )
                .await
            {
                warn!(target: "neo", error = %err, "failed to forward headers to blockchain");
            }

            if let Err(err) = self.system.task_manager.tell_from(
                TaskManagerCommand::Headers { headers },
                Some(ctx.self_ref()),
            ) {
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
        if let Some(endpoint) = address.endpoint() {
            if endpoint.port() > 0 && seen.insert(endpoint) {
                endpoints.push(endpoint);
            }
        }
    }

    endpoints
}

#[cfg(test)]
mod tests {
    use super::{collect_reachable_endpoints, normalize_request};
    use crate::network::p2p::capabilities::tcp_server;
    use crate::network::p2p::payloads::network_address_with_time::NetworkAddressWithTime;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    fn address(ip: [u8; 4], port: u16) -> NetworkAddressWithTime {
        NetworkAddressWithTime::new(0, IpAddr::V4(Ipv4Addr::from(ip)), vec![tcp_server(port)])
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
