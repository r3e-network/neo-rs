use crate::neo_io::{BinaryWriter, IoResult, MemoryReader, Serializable};
use crate::network::p2p::message_command::MessageCommand;
use crate::network::p2p::payloads::{
    AddrPayload, Block, ExtensiblePayload, FilterAddPayload, FilterLoadPayload,
    GetBlockByIndexPayload, GetBlocksPayload, HeadersPayload, InvPayload, MerkleBlockPayload,
    PingPayload, Transaction, VersionPayload,
};
use crate::network::{NetworkError, NetworkResult};

pub(super) fn serialize_payload<T>(payload: &T) -> NetworkResult<Vec<u8>>
where
    T: PayloadSerializable,
{
    payload
        .serialize_to_vec()
        .map_err(|e| NetworkError::InvalidMessage(e.to_string()))
}

type PayloadResult<T> = IoResult<T>;

pub(super) fn deserialize_payload<T>(bytes: &[u8]) -> NetworkResult<T>
where
    T: PayloadDeserializable,
{
    let mut reader = MemoryReader::new(bytes);
    let payload =
        T::deserialize(&mut reader).map_err(|e| NetworkError::InvalidMessage(e.to_string()))?;
    if reader.remaining() != 0 {
        return Err(NetworkError::InvalidMessage(
            "Trailing bytes present after payload deserialization".to_string(),
        ));
    }
    Ok(payload)
}

pub(super) fn ensure_empty(command: MessageCommand, bytes: &[u8]) -> NetworkResult<()> {
    if bytes.is_empty() {
        Ok(())
    } else {
        Err(NetworkError::InvalidMessage(format!(
            "Command {:?} does not carry a payload but {} byte(s) were provided",
            command,
            bytes.len()
        )))
    }
}

pub(super) trait PayloadSerializable {
    fn serialize_to_vec(&self) -> IoResult<Vec<u8>>;
}

pub(super) trait PayloadDeserializable: Sized {
    fn deserialize(reader: &mut MemoryReader) -> PayloadResult<Self>;
}

macro_rules! impl_payload_codec {
    ($type:ty) => {
        impl PayloadSerializable for $type {
            fn serialize_to_vec(&self) -> IoResult<Vec<u8>> {
                let mut writer = BinaryWriter::new();
                Serializable::serialize(self, &mut writer)?;
                Ok(writer.into_bytes())
            }
        }

        impl PayloadDeserializable for $type {
            fn deserialize(reader: &mut MemoryReader) -> PayloadResult<Self> {
                <$type as Serializable>::deserialize(reader)
            }
        }
    };
}

// Implement the codec helpers for every payload that already satisfies the
// `crate::neo_io::Serializable` contract.
impl_payload_codec!(VersionPayload);
impl_payload_codec!(AddrPayload);
impl_payload_codec!(PingPayload);
impl_payload_codec!(GetBlockByIndexPayload);
impl_payload_codec!(HeadersPayload);
impl_payload_codec!(GetBlocksPayload);
impl_payload_codec!(InvPayload);
impl_payload_codec!(Transaction);
impl_payload_codec!(Block);
impl_payload_codec!(ExtensiblePayload);
impl_payload_codec!(FilterLoadPayload);
impl_payload_codec!(FilterAddPayload);
impl_payload_codec!(MerkleBlockPayload);
