#[doc(hidden)]
#[macro_export]
macro_rules! __protocol_enum_display {
    ($variant:ident) => {
        stringify!($variant)
    };
    ($variant:ident, $display:expr) => {
        $display
    };
}

/// Generates a `#[repr(u8)]` protocol enum with standard boilerplate.
///
/// Each variant is specified as `Name = BYTE_VALUE`, optionally preceded by
/// attributes like `#[default]` or `#[doc = "..."]`. The macro generates:
/// - The enum definition with `#[repr(u8)]`
/// - `to_byte() -> u8`, `from_byte(u8) -> Option<Self>`, `as_str() -> &str`
/// - `Display`, `Serialize`, `Deserialize` trait implementations
///
/// Additional methods (like `is_success()`) must be added in a separate `impl` block.
///
/// # Example
///
/// ```rust,ignore
/// use neo_primitives::protocol_enum;
///
/// protocol_enum! {
///     #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
///     pub OracleResponseCode {
///         Success = 0x00,
///         ProtocolNotSupported = 0x10,
///         Error = 0xff,
///     }
/// }
/// ```
///
/// Custom display names can be supplied for protocol enums whose canonical
/// string form does not match the Rust variant name:
///
/// ```rust,ignore
/// protocol_enum! {
///     #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
///     pub NamedCurveHash {
///         Secp256k1SHA256 = 0x16 => "secp256k1SHA256",
///     }
/// }
/// ```
#[macro_export]
macro_rules! protocol_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr $(=> $display:expr)?
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[repr(u8)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant = $byte,
            )+
        }

        impl $name {
            /// Returns the protocol byte assigned to this enum value.
            #[must_use]
            #[inline]
            pub const fn to_byte(self) -> u8 {
                self as u8
            }

            /// Parses this enum from its protocol byte.
            #[must_use]
            pub const fn from_byte(value: u8) -> Option<Self> {
                match value {
                    $(
                        $byte => Some(Self::$variant),
                    )+
                    _ => None,
                }
            }

            /// Returns the canonical display name for this enum value.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $crate::__protocol_enum_display!($variant $(, $display)?),
                    )+
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, "{}", self.as_str())
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S: ::serde::Serializer>(
                &self,
                serializer: S,
            ) -> ::std::result::Result<S::Ok, S::Error> {
                serializer.serialize_u8(self.to_byte())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D: ::serde::Deserializer<'de>>(
                deserializer: D,
            ) -> ::std::result::Result<Self, D::Error> {
                let byte = <u8 as ::serde::Deserialize>::deserialize(deserializer)?;
                Self::from_byte(byte).ok_or_else(|| {
                    ::serde::de::Error::custom(format!(
                        "Invalid {} byte: {}",
                        stringify!($name),
                        byte
                    ))
                })
            }
        }
    };
}

/// Generates a `#[repr(u8)]` protocol enum with byte helpers and `Display`.
///
/// Unlike [`protocol_enum!`], this macro deliberately does not implement
/// `Serialize` or `Deserialize`. Use it for public enums that already expose
/// serde's derived enum-name shape while still needing protocol byte helpers.
#[macro_export]
macro_rules! protocol_enum_repr {
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr $(=> $display:expr)?
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        #[repr(u8)]
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant = $byte,
            )+
        }

        impl $name {
            /// Returns the protocol byte assigned to this enum value.
            #[must_use]
            #[inline]
            pub const fn to_byte(self) -> u8 {
                self as u8
            }

            /// Parses this enum from its protocol byte.
            #[must_use]
            pub const fn from_byte(value: u8) -> Option<Self> {
                match value {
                    $(
                        $byte => Some(Self::$variant),
                    )+
                    _ => None,
                }
            }

            /// Returns the canonical display name for this enum value.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $crate::__protocol_enum_display!($variant $(, $display)?),
                    )+
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, "{}", self.as_str())
            }
        }
    };
}

/// Generates a protocol enum whose unknown byte values must round-trip.
///
/// Use this for protocol surfaces where future or private extension bytes are
/// valid on the wire. The macro generates:
/// - The enum definition, including one tuple-style unknown variant.
/// - `to_byte() -> u8`, `as_byte() -> u8`, `from_byte(u8) -> Self`.
/// - `as_str() -> &str` and `is_known() -> bool`.
///
/// `Display`, `FromStr`, and `serde` are intentionally left to the caller so
/// each protocol surface can preserve its existing public/API encoding shape.
/// Callers that need to preserve an existing fallible `from_byte` API can add
/// `from_byte = from_byte_unchecked;` before `unknown` and wrap the generated
/// helper in their own impl.
///
/// # Example
///
/// ```rust,ignore
/// use neo_primitives::protocol_enum_with_unknown;
///
/// protocol_enum_with_unknown! {
///     #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
///     pub MessageCommand {
///         unknown
///         /// Unknown command byte.
///         Unknown(u8) => "unknown";
///
///         Version = 0x00 => "version",
///         Transaction = 0x2b => "tx",
///     }
/// }
/// ```
#[macro_export]
macro_rules! protocol_enum_with_unknown {
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            from_byte = $from_byte:ident;
            unknown
            $(#[$unknown_meta:meta])*
            $unknown:ident(u8) => $unknown_display:expr;

            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr $(=> $display:expr)?
            ),+ $(,)?
        }
    ) => {
        $(#[$enum_meta])*
        $vis enum $name {
            $(
                $(#[$variant_meta])*
                $variant,
            )+
            $(#[$unknown_meta])*
            $unknown(u8),
        }

        impl $name {
            /// Returns the protocol byte assigned to this enum value.
            #[must_use]
            #[inline]
            pub const fn to_byte(self) -> u8 {
                match self {
                    $(
                        Self::$variant => $byte,
                    )+
                    Self::$unknown(value) => value,
                }
            }

            /// Alias for [`Self::to_byte`]; retained for backward compatibility.
            #[must_use]
            #[inline]
            pub const fn as_byte(self) -> u8 {
                self.to_byte()
            }

            /// Parses this enum from its protocol byte, preserving unknown bytes.
            #[must_use]
            pub const fn $from_byte(value: u8) -> Self {
                match value {
                    $(
                        $byte => Self::$variant,
                    )+
                    other => Self::$unknown(other),
                }
            }

            /// Returns the canonical display name for this enum value.
            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(
                        Self::$variant => $crate::__protocol_enum_display!($variant $(, $display)?),
                    )+
                    Self::$unknown(_) => $unknown_display,
                }
            }

            /// Returns `true` for known protocol variants.
            #[must_use]
            pub const fn is_known(self) -> bool {
                !matches!(self, Self::$unknown(_))
            }
        }
    };
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            unknown
            $(#[$unknown_meta:meta])*
            $unknown:ident(u8) => $unknown_display:expr;

            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr $(=> $display:expr)?
            ),+ $(,)?
        }
    ) => {
        $crate::protocol_enum_with_unknown! {
            $(#[$enum_meta])*
            $vis $name {
                from_byte = from_byte;
                unknown
                $(#[$unknown_meta])*
                $unknown(u8) => $unknown_display;

                $(
                    $(#[$variant_meta])*
                    $variant = $byte $(=> $display)?
                ),+
            }
        }
    };
}

/// Generates Neo P2P `MessageCommand` definitions from one canonical command table.
///
/// `neo-core` and `neo-p2p` intentionally keep crate-local error types, so this
/// macro has two public API modes:
/// - `from_byte = result` keeps a fallible `from_byte(u8) -> Result<Self, E>`.
/// - `from_byte = infallible` keeps `from_byte(u8) -> Self`.
#[macro_export]
macro_rules! p2p_message_command {
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            error = $error_ty:ty;
            parse_error = $parse_error:expr;
            from_byte = result;
            extended_aliases = true;
        }
    ) => {
        $crate::protocol_enum_with_unknown! {
            $(#[$enum_meta])*
            $vis $name {
                from_byte = from_byte_unchecked;
                unknown
                /// Command value that is not recognised by this implementation.
                Unknown(u8) => "unknown";

                /// Version handshake message.
                Version = 0x00 => "version",
                /// Version acknowledgment message.
                Verack = 0x01 => "verack",
                /// Request for peer addresses.
                GetAddr = 0x10 => "getaddr",
                /// Response with peer addresses.
                Addr = 0x11 => "addr",
                /// Ping message for keepalive.
                Ping = 0x18 => "ping",
                /// Pong response to ping.
                Pong = 0x19 => "pong",
                /// Request for block headers.
                GetHeaders = 0x20 => "getheaders",
                /// Response with block headers.
                Headers = 0x21 => "headers",
                /// Request for block hashes.
                GetBlocks = 0x24 => "getblocks",
                /// Request for mempool transactions.
                Mempool = 0x25 => "mempool",
                /// Inventory announcement.
                Inv = 0x27 => "inv",
                /// Request for specific data.
                GetData = 0x28 => "getdata",
                /// Request block by index.
                GetBlockByIndex = 0x29 => "getblkbyidx",
                /// Data not found response.
                NotFound = 0x2a => "notfound",
                /// Transaction payload.
                Transaction = 0x2b => "tx",
                /// Block payload.
                Block = 0x2c => "block",
                /// Extensible message payload.
                Extensible = 0x2e => "extensible",
                /// Rejection message.
                Reject = 0x2f => "reject",
                /// Load bloom filter.
                FilterLoad = 0x30 => "filterload",
                /// Add to bloom filter.
                FilterAdd = 0x31 => "filteradd",
                /// Clear bloom filter.
                FilterClear = 0x32 => "filterclear",
                /// Merkle block for SPV.
                MerkleBlock = 0x38 => "merkleblock",
                /// Alert message.
                Alert = 0x40 => "alert",
            }
        }

        impl $name {
            /// Creates a command value from its byte representation.
            pub fn from_byte(byte: u8) -> ::std::result::Result<Self, $error_ty> {
                Ok(Self::from_byte_unchecked(byte))
            }

            /// Parses a command from its textual form.
            pub fn parse_str(s: &str) -> ::std::result::Result<Self, $error_ty> {
                match s {
                    "version" => Ok(Self::Version),
                    "verack" => Ok(Self::Verack),
                    "getaddr" => Ok(Self::GetAddr),
                    "addr" => Ok(Self::Addr),
                    "ping" => Ok(Self::Ping),
                    "pong" => Ok(Self::Pong),
                    "getheaders" => Ok(Self::GetHeaders),
                    "headers" => Ok(Self::Headers),
                    "getblocks" => Ok(Self::GetBlocks),
                    "mempool" => Ok(Self::Mempool),
                    "inv" => Ok(Self::Inv),
                    "getdata" => Ok(Self::GetData),
                    "getblkbyidx" => Ok(Self::GetBlockByIndex),
                    "notfound" => Ok(Self::NotFound),
                    "tx" => Ok(Self::Transaction),
                    "block" => Ok(Self::Block),
                    "extensible" => Ok(Self::Extensible),
                    "reject" => Ok(Self::Reject),
                    "filterload" => Ok(Self::FilterLoad),
                    "filteradd" => Ok(Self::FilterAdd),
                    "filterclear" => Ok(Self::FilterClear),
                    "merkleblock" => Ok(Self::MerkleBlock),
                    "alert" => Ok(Self::Alert),
                    "versionwithpayload" => Ok(Self::Unknown(0x55)),
                    "extended83" => Ok(Self::Unknown(0x83)),
                    "extended85" => Ok(Self::Unknown(0x85)),
                    "extended86" => Ok(Self::Unknown(0x86)),
                    "extendedbb" => Ok(Self::Unknown(0xbb)),
                    "extendedbd" => Ok(Self::Unknown(0xbd)),
                    "extendedbf" => Ok(Self::Unknown(0xbf)),
                    "extendedc0" => Ok(Self::Unknown(0xc0)),
                    "unknown" => Ok(Self::Unknown(0xff)),
                    other => Err(($parse_error)(other)),
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, "{}", self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $error_ty;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                Self::parse_str(s)
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                serializer.serialize_u8(self.to_byte())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let value = <u8 as ::serde::Deserialize>::deserialize(deserializer)?;
                Self::from_byte(value)
                    .map_err(|error| <D::Error as ::serde::de::Error>::custom(error))
            }
        }
    };
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            error = $error_ty:ty;
            parse_error = $parse_error:expr;
            from_byte = infallible;
            extended_aliases = false;
        }
    ) => {
        $crate::protocol_enum_with_unknown! {
            $(#[$enum_meta])*
            $vis $name {
                from_byte = from_byte_unchecked;
                unknown
                /// Command value that is not recognised by this implementation.
                Unknown(u8) => "unknown";

                /// Version handshake message.
                Version = 0x00 => "version",
                /// Version acknowledgment message.
                Verack = 0x01 => "verack",
                /// Request for peer addresses.
                GetAddr = 0x10 => "getaddr",
                /// Response with peer addresses.
                Addr = 0x11 => "addr",
                /// Ping message for keepalive.
                Ping = 0x18 => "ping",
                /// Pong response to ping.
                Pong = 0x19 => "pong",
                /// Request for block headers.
                GetHeaders = 0x20 => "getheaders",
                /// Response with block headers.
                Headers = 0x21 => "headers",
                /// Request for block hashes.
                GetBlocks = 0x24 => "getblocks",
                /// Request for mempool transactions.
                Mempool = 0x25 => "mempool",
                /// Inventory announcement.
                Inv = 0x27 => "inv",
                /// Request for specific data.
                GetData = 0x28 => "getdata",
                /// Request block by index.
                GetBlockByIndex = 0x29 => "getblkbyidx",
                /// Data not found response.
                NotFound = 0x2a => "notfound",
                /// Transaction payload.
                Transaction = 0x2b => "tx",
                /// Block payload.
                Block = 0x2c => "block",
                /// Extensible message payload.
                Extensible = 0x2e => "extensible",
                /// Rejection message.
                Reject = 0x2f => "reject",
                /// Load bloom filter.
                FilterLoad = 0x30 => "filterload",
                /// Add to bloom filter.
                FilterAdd = 0x31 => "filteradd",
                /// Clear bloom filter.
                FilterClear = 0x32 => "filterclear",
                /// Merkle block for SPV.
                MerkleBlock = 0x38 => "merkleblock",
                /// Alert message.
                Alert = 0x40 => "alert",
            }
        }

        impl $name {
            /// Creates a command value from its byte representation.
            pub const fn from_byte(byte: u8) -> Self {
                Self::from_byte_unchecked(byte)
            }

            /// Parses a command from its textual form.
            pub fn parse_str(s: &str) -> ::std::result::Result<Self, $error_ty> {
                match s {
                    "version" => Ok(Self::Version),
                    "verack" => Ok(Self::Verack),
                    "getaddr" => Ok(Self::GetAddr),
                    "addr" => Ok(Self::Addr),
                    "ping" => Ok(Self::Ping),
                    "pong" => Ok(Self::Pong),
                    "getheaders" => Ok(Self::GetHeaders),
                    "headers" => Ok(Self::Headers),
                    "getblocks" => Ok(Self::GetBlocks),
                    "mempool" => Ok(Self::Mempool),
                    "inv" => Ok(Self::Inv),
                    "getdata" => Ok(Self::GetData),
                    "getblkbyidx" => Ok(Self::GetBlockByIndex),
                    "notfound" => Ok(Self::NotFound),
                    "tx" => Ok(Self::Transaction),
                    "block" => Ok(Self::Block),
                    "extensible" => Ok(Self::Extensible),
                    "reject" => Ok(Self::Reject),
                    "filterload" => Ok(Self::FilterLoad),
                    "filteradd" => Ok(Self::FilterAdd),
                    "filterclear" => Ok(Self::FilterClear),
                    "merkleblock" => Ok(Self::MerkleBlock),
                    "alert" => Ok(Self::Alert),
                    "unknown" => Ok(Self::Unknown(0xff)),
                    other => Err(($parse_error)(other)),
                }
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                write!(f, "{}", self.as_str())
            }
        }

        impl ::std::str::FromStr for $name {
            type Err = $error_ty;

            fn from_str(s: &str) -> ::std::result::Result<Self, Self::Err> {
                Self::parse_str(s)
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                serializer.serialize_u8(self.to_byte())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let value = <u8 as ::serde::Deserialize>::deserialize(deserializer)?;
                Ok(Self::from_byte(value))
            }
        }
    };
}

/// Generates the Neo P2P message flag wrapper while preserving unknown bits.
///
/// Neo treats message flags as a raw byte with bit flags. Only bit `0x01`
/// currently means "compressed"; all other bits must round-trip for forward
/// compatibility.
#[macro_export]
macro_rules! protocol_message_flags {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            warn_target = $warn_target:literal;
            from_byte = $from_byte:ident;
        }
    ) => {
        $(#[$meta])*
        $vis struct $name(u8);

        impl $name {
            /// No flags are set.
            pub const NONE: Self = Self(0x00);
            /// The payload is compressed.
            pub const COMPRESSED: Self = Self(0x01);

            /// Creates a new flag set with the given raw value.
            #[must_use]
            pub const fn new(value: u8) -> Self {
                Self(value)
            }

            /// Converts the flags to their byte representation.
            #[must_use]
            #[inline]
            pub const fn to_byte(self) -> u8 {
                self.0
            }

            /// Alias for [`Self::to_byte`]; retained for backward compatibility.
            #[must_use]
            #[inline]
            pub const fn as_byte(self) -> u8 {
                self.to_byte()
            }

            /// Parses the flags from their byte representation.
            ///
            /// This method accepts any byte value, logging a warning for unknown
            /// bits but preserving them for forward compatibility.
            #[must_use]
            pub fn $from_byte(byte: u8) -> Self {
                if byte & !Self::COMPRESSED.0 != 0 {
                    ::tracing::warn!(
                        target: $warn_target,
                        "message flags include unknown bits (0x{:02x}); preserving raw value",
                        byte
                    );
                }
                Self(byte)
            }

            /// Returns `true` when the compressed flag is set.
            #[must_use]
            #[inline]
            pub const fn is_compressed(self) -> bool {
                self.0 & Self::COMPRESSED.0 != 0
            }

            /// Sets the compressed flag.
            pub fn set_compressed(&mut self, compressed: bool) {
                if compressed {
                    self.0 |= Self::COMPRESSED.0;
                } else {
                    self.0 &= !Self::COMPRESSED.0;
                }
            }

            /// Returns a new flag set with the compressed flag updated.
            #[must_use]
            pub fn with_compressed(mut self, compressed: bool) -> Self {
                self.set_compressed(compressed);
                self
            }
        }

        impl ::serde::Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> ::std::result::Result<S::Ok, S::Error>
            where
                S: ::serde::Serializer,
            {
                serializer.serialize_u8(self.to_byte())
            }
        }

        impl<'de> ::serde::Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> ::std::result::Result<Self, D::Error>
            where
                D: ::serde::Deserializer<'de>,
            {
                let value = <u8 as ::serde::Deserialize>::deserialize(deserializer)?;
                Ok(Self(value))
            }
        }
    };
}
