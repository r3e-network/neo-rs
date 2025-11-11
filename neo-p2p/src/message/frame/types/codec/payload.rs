use neo_base::{encoding::DecodeError, Bytes, NeoDecode, NeoEncode, NeoRead, SliceReader};
use std::vec::Vec;

use crate::message::{
    command::MessageCommand,
    types::{
        AddressPayload, AlertPayload, FilterAddPayload, FilterLoadPayload, GetBlockByIndexPayload,
        GetBlocksPayload, HeadersPayload, InventoryPayload, MerkleBlockPayload, PayloadWithData,
        PingPayload, RejectPayload, VersionPayload,
    },
};

use super::super::Message;

pub(super) fn encode_payload(message: &Message, buf: &mut Vec<u8>) {
    match message {
        Message::Version(payload) => payload.neo_encode(buf),
        Message::Verack | Message::GetAddr | Message::Mempool => {}
        Message::Address(payload) => payload.neo_encode(buf),
        Message::Ping(payload) | Message::Pong(payload) => payload.neo_encode(buf),
        Message::GetHeaders(payload) | Message::GetBlockByIndex(payload) => payload.neo_encode(buf),
        Message::Headers(payload) => payload.neo_encode(buf),
        Message::GetBlocks(payload) => payload.neo_encode(buf),
        Message::Inventory(payload) | Message::GetData(payload) | Message::NotFound(payload) => {
            payload.neo_encode(buf)
        }
        Message::Block(payload) | Message::Transaction(payload) => payload.neo_encode(buf),
        Message::Extensible(payload) => payload.neo_encode(buf),
        Message::Reject(payload) => payload.neo_encode(buf),
        Message::FilterLoad(payload) => payload.neo_encode(buf),
        Message::FilterAdd(payload) => payload.neo_encode(buf),
        Message::FilterClear => {}
        Message::MerkleBlock(payload) => payload.neo_encode(buf),
        Message::Alert(payload) => payload.neo_encode(buf),
    }
}

pub(super) fn decode_payload(
    command: MessageCommand,
    payload: &[u8],
) -> Result<Message, DecodeError> {
    use MessageCommand::*;
    let mut reader = SliceReader::new(payload);
    let message = match command {
        Version => Message::Version(VersionPayload::neo_decode(&mut reader)?),
        Verack => {
            if reader.remaining() != 0 {
                return Err(DecodeError::InvalidValue("verack payload"));
            }
            Message::Verack
        }
        GetAddr => {
            if reader.remaining() != 0 {
                return Err(DecodeError::InvalidValue("getaddr payload"));
            }
            Message::GetAddr
        }
        Addr => Message::Address(AddressPayload::neo_decode(&mut reader)?),
        Ping => Message::Ping(PingPayload::neo_decode(&mut reader)?),
        Pong => Message::Pong(PingPayload::neo_decode(&mut reader)?),
        GetHeaders => Message::GetHeaders(GetBlockByIndexPayload::neo_decode(&mut reader)?),
        Headers => Message::Headers(HeadersPayload::neo_decode(&mut reader)?),
        GetBlocks => Message::GetBlocks(GetBlocksPayload::neo_decode(&mut reader)?),
        GetBlockByIndex => {
            Message::GetBlockByIndex(GetBlockByIndexPayload::neo_decode(&mut reader)?)
        }
        Mempool => {
            if reader.remaining() != 0 {
                return Err(DecodeError::InvalidValue("mempool payload"));
            }
            Message::Mempool
        }
        Inv => Message::Inventory(InventoryPayload::neo_decode(&mut reader)?),
        GetData => Message::GetData(InventoryPayload::neo_decode(&mut reader)?),
        NotFound => Message::NotFound(InventoryPayload::neo_decode(&mut reader)?),
        Block => Message::Block(PayloadWithData::neo_decode(&mut reader)?),
        Transaction => Message::Transaction(PayloadWithData::neo_decode(&mut reader)?),
        Extensible => Message::Extensible(Bytes::neo_decode(&mut reader)?),
        Reject => Message::Reject(RejectPayload::neo_decode(&mut reader)?),
        FilterLoad => Message::FilterLoad(FilterLoadPayload::neo_decode(&mut reader)?),
        FilterAdd => Message::FilterAdd(FilterAddPayload::neo_decode(&mut reader)?),
        FilterClear => {
            if reader.remaining() != 0 {
                return Err(DecodeError::InvalidValue("filterclear payload"));
            }
            Message::FilterClear
        }
        MerkleBlock => Message::MerkleBlock(MerkleBlockPayload::neo_decode(&mut reader)?),
        Alert => Message::Alert(AlertPayload::neo_decode(&mut reader)?),
    };

    if reader.remaining() != 0 {
        return Err(DecodeError::InvalidValue("message payload trailing bytes"));
    }

    Ok(message)
}
