use crate::network::p2p::{
    payloads::{
        get_block_by_index_payload::GetBlockByIndexPayload, inv_payload::InvPayload, InventoryType,
    },
    NetworkMessage, ProtocolMessage, RemoteNodeCommand,
};
use crate::runtime::{ActorRef, AkkaResult};
use crate::UInt256;
use tracing::warn;

pub(super) fn send_get_data_groups<I>(
    actor: &ActorRef,
    inventory_type: InventoryType,
    hashes: I,
    failure_message: &'static str,
) where
    I: IntoIterator<Item = UInt256>,
{
    for group in InvPayload::create_group(inventory_type, hashes) {
        if let Err(error) = send_protocol_message(actor, ProtocolMessage::GetData(group)) {
            warn!(
                target: "neo",
                actor = %actor.path(),
                %error,
                "{}",
                failure_message
            );
        }
    }
}

pub(super) fn send_get_headers(actor: &ActorRef, start_index: u32) -> AkkaResult<()> {
    let payload = GetBlockByIndexPayload::create(start_index, -1);
    send_protocol_message(actor, ProtocolMessage::GetHeaders(payload))
}

pub(super) fn send_get_blocks_by_index(
    actor: &ActorRef,
    start_index: u32,
    count: i16,
) -> AkkaResult<()> {
    let payload = GetBlockByIndexPayload::create(start_index, count);
    send_protocol_message(actor, ProtocolMessage::GetBlockByIndex(payload))
}

pub(super) fn send_mempool(actor: &ActorRef) -> AkkaResult<()> {
    send_protocol_message(actor, ProtocolMessage::Mempool)
}

pub(super) fn disconnect(actor: &ActorRef, reason: String) -> AkkaResult<()> {
    actor.tell(RemoteNodeCommand::Disconnect { reason })
}

fn send_protocol_message(actor: &ActorRef, message: ProtocolMessage) -> AkkaResult<()> {
    actor.tell(RemoteNodeCommand::Send(NetworkMessage::new(message)))
}
