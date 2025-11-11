use neo_base::{encoding::DecodeError, Bytes, NeoDecode, NeoEncode, NeoRead, NeoWrite};

use crate::message::{
    command::MessageCommand,
    types::{
        AddressPayload, AlertPayload, FilterAddPayload, FilterLoadPayload, GetBlockByIndexPayload,
        GetBlocksPayload, HeadersPayload, InventoryPayload, MerkleBlockPayload, PayloadWithData,
        PingPayload, RejectPayload, VersionPayload,
    },
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Message {
    Version(VersionPayload),
    Verack,
    GetAddr,
    Address(AddressPayload),
    Ping(PingPayload),
    Pong(PingPayload),
    GetHeaders(GetBlockByIndexPayload),
    Headers(HeadersPayload),
    GetBlocks(GetBlocksPayload),
    GetBlockByIndex(GetBlockByIndexPayload),
    Mempool,
    Inventory(InventoryPayload),
    GetData(InventoryPayload),
    NotFound(InventoryPayload),
    Block(PayloadWithData),
    Transaction(PayloadWithData),
    Extensible(Bytes),
    Reject(RejectPayload),
    FilterLoad(FilterLoadPayload),
    FilterAdd(FilterAddPayload),
    FilterClear,
    MerkleBlock(MerkleBlockPayload),
    Alert(AlertPayload),
}

impl Message {
    pub fn command(&self) -> MessageCommand {
        match self {
            Message::Version(_) => MessageCommand::Version,
            Message::Verack => MessageCommand::Verack,
            Message::GetAddr => MessageCommand::GetAddr,
            Message::Address(_) => MessageCommand::Addr,
            Message::Ping(_) => MessageCommand::Ping,
            Message::Pong(_) => MessageCommand::Pong,
            Message::GetHeaders(_) => MessageCommand::GetHeaders,
            Message::Headers(_) => MessageCommand::Headers,
            Message::GetBlocks(_) => MessageCommand::GetBlocks,
            Message::GetBlockByIndex(_) => MessageCommand::GetBlockByIndex,
            Message::Mempool => MessageCommand::Mempool,
            Message::Inventory(_) => MessageCommand::Inv,
            Message::GetData(_) => MessageCommand::GetData,
            Message::NotFound(_) => MessageCommand::NotFound,
            Message::Block(_) => MessageCommand::Block,
            Message::Transaction(_) => MessageCommand::Transaction,
            Message::Extensible(_) => MessageCommand::Extensible,
            Message::Reject(_) => MessageCommand::Reject,
            Message::FilterLoad(_) => MessageCommand::FilterLoad,
            Message::FilterAdd(_) => MessageCommand::FilterAdd,
            Message::FilterClear => MessageCommand::FilterClear,
            Message::MerkleBlock(_) => MessageCommand::MerkleBlock,
            Message::Alert(_) => MessageCommand::Alert,
        }
    }

    pub fn command_name(&self) -> &'static str {
        self.command().as_str()
    }

    pub fn neo_encode_with_compression<W: NeoWrite>(
        &self,
        writer: &mut W,
        allow_compression: bool,
    ) {
        super::codec::encode_inner(self, writer, allow_compression);
    }
}

impl NeoEncode for Message {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        self.neo_encode_with_compression(writer, true);
    }
}

impl NeoDecode for Message {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        super::codec::decode_inner(reader)
    }
}
