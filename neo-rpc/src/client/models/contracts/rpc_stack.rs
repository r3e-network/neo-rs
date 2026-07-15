//! RPC stack item representation (`RpcStack`).

use neo_error::{CoreError, CoreResult};
use neo_serialization::json::{JObject, JToken};
use num_bigint::BigInt;

/// Immutable stack item received from or sent to a remote JSON-RPC node.
///
/// This is a transport DTO, not a NeoVM runtime value. In particular, pointer
/// positions and interop interfaces are opaque remote data and cannot enter
/// consensus execution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RpcStackItem {
    /// NeoVM `Any` / null.
    Null,
    /// Boolean value.
    Boolean(bool),
    /// Arbitrary-precision NeoVM integer.
    Integer(BigInt),
    /// Immutable byte string.
    ByteString(Vec<u8>),
    /// Mutable-buffer wire value. Mutability is not modeled by this DTO.
    Buffer(Vec<u8>),
    /// Array value.
    Array(Vec<Self>),
    /// Struct value.
    Struct(Vec<Self>),
    /// Ordered map entries.
    Map(Vec<(Self, Self)>),
    /// Opaque instruction position in the remote VM.
    Pointer(u32),
    /// Opaque remote interop interface, including iterator metadata.
    InteropInterface {
        /// Interface name, such as `IIterator`.
        interface: Option<String>,
        /// Opaque RPC-side handle.
        id: Option<String>,
    },
}

impl RpcStackItem {
    /// Returns the bytes carried by byte-like RPC values.
    #[must_use]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match self {
            Self::ByteString(bytes) | Self::Buffer(bytes) => Some(bytes),
            _ => None,
        }
    }
}

/// RPC stack item representation matching C# `RpcStack`
#[derive(Debug, Clone)]
pub struct RpcStack {
    /// Stack item type
    pub item_type: String,

    /// Stack item value
    pub value: JToken,
}

impl RpcStack {
    /// Creates from JSON
    /// Matches C# `FromJson`
    pub fn from_json(json: &JObject) -> CoreResult<Self> {
        let item_type = json
            .get("type")
            .and_then(neo_serialization::json::JToken::as_string)
            .ok_or_else(|| CoreError::other("Missing or invalid 'type' field"))?;

        let value = json
            .get("value")
            .ok_or_else(|| CoreError::other("Missing 'value' field"))?
            .clone();

        Ok(Self { item_type, value })
    }

    /// Converts to JSON
    /// Matches C# `ToJson`
    #[must_use]
    pub fn to_json(&self) -> JObject {
        let mut json = JObject::new();
        json.insert("type".to_string(), JToken::String(self.item_type.clone()));
        json.insert("value".to_string(), self.value.clone());
        json
    }
}
