/// Generates Neo P2P `MessageCommand` definitions from one canonical command table.
///
/// `neo-core` and `neo-p2p` intentionally keep crate-local error types, so this
/// macro has two public API modes:
/// - `from_byte = result` keeps a fallible `from_byte(u8) -> Result<Self, E>`.
/// - `from_byte = infallible` keeps `from_byte(u8) -> Self`.
#[doc(hidden)]
#[macro_export]
macro_rules! __p2p_message_command_table {
    ($callback:ident; $($args:tt)*) => {
        $crate::$callback! {
            $($args)*
            ;
            {
                #[doc = "Version handshake message."]
                Version = 0x00 => "version",
                #[doc = "Version acknowledgment message."]
                Verack = 0x01 => "verack",
                #[doc = "Request for peer addresses."]
                GetAddr = 0x10 => "getaddr",
                #[doc = "Response with peer addresses."]
                Addr = 0x11 => "addr",
                #[doc = "Ping message for keepalive."]
                Ping = 0x18 => "ping",
                #[doc = "Pong response to ping."]
                Pong = 0x19 => "pong",
                #[doc = "Request for block headers."]
                GetHeaders = 0x20 => "getheaders",
                #[doc = "Response with block headers."]
                Headers = 0x21 => "headers",
                #[doc = "Request for block hashes."]
                GetBlocks = 0x24 => "getblocks",
                #[doc = "Request for mempool transactions."]
                Mempool = 0x25 => "mempool",
                #[doc = "Inventory announcement."]
                Inv = 0x27 => "inv",
                #[doc = "Request for specific data."]
                GetData = 0x28 => "getdata",
                #[doc = "Request block by index."]
                GetBlockByIndex = 0x29 => "getblkbyidx",
                #[doc = "Data not found response."]
                NotFound = 0x2a => "notfound",
                #[doc = "Transaction payload."]
                Transaction = 0x2b => "tx",
                #[doc = "Block payload."]
                Block = 0x2c => "block",
                #[doc = "Extensible message payload."]
                Extensible = 0x2e => "extensible",
                #[doc = "Rejection message."]
                Reject = 0x2f => "reject",
                #[doc = "Load bloom filter."]
                FilterLoad = 0x30 => "filterload",
                #[doc = "Add to bloom filter."]
                FilterAdd = 0x31 => "filteradd",
                #[doc = "Clear bloom filter."]
                FilterClear = 0x32 => "filterclear",
                #[doc = "Merkle block for SPV."]
                MerkleBlock = 0x38 => "merkleblock",
                #[doc = "Alert message."]
                Alert = 0x40 => "alert",
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __p2p_message_command_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident
    ) => {
        $crate::__p2p_message_command_table! {
            __p2p_message_command_enum_from_table;
            $(#[$enum_meta])*
            $vis $name
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __p2p_message_command_enum_from_table {
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident
        ;
        {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr_2021 => $display:expr_2021
            ),+ $(,)?
        }
    ) => {
        $crate::protocol_enum_with_unknown! {
            $(#[$enum_meta])*
            $vis $name {
                from_byte = from_byte_unchecked;
                unknown
                /// Command value that is not recognised by this implementation.
                Unknown(u8) => "unknown";

                $(
                    $(#[$variant_meta])*
                    $variant = $byte => $display
                ),+
            }
        }
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __p2p_message_command_parse {
    ($name:ident, $source:expr_2021, $parse_error:expr_2021, $extended_aliases:expr_2021) => {{
        $crate::__p2p_message_command_table! {
            __p2p_message_command_parse_from_table;
            $name, $source, $parse_error, $extended_aliases
        }
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __p2p_message_command_parse_from_table {
    (
        $name:ident, $source:expr_2021, $parse_error:expr_2021, $extended_aliases:expr_2021
        ;
        {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr_2021 => $display:expr_2021
            ),+ $(,)?
        }
    ) => {{
        match $source {
            $(
                $display => Ok($name::$variant),
            )+
            "unknown" => Ok($name::Unknown(0xff)),
            other if $extended_aliases => match other {
                "versionwithpayload" => Ok($name::Unknown(0x55)),
                "extended83" => Ok($name::Unknown(0x83)),
                "extended85" => Ok($name::Unknown(0x85)),
                "extended86" => Ok($name::Unknown(0x86)),
                "extendedbb" => Ok($name::Unknown(0xbb)),
                "extendedbd" => Ok($name::Unknown(0xbd)),
                "extendedbf" => Ok($name::Unknown(0xbf)),
                "extendedc0" => Ok($name::Unknown(0xc0)),
                other => Err(($parse_error)(other)),
            },
            other => Err(($parse_error)(other)),
        }
    }};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __p2p_message_command_shared_impls {
    ($name:ident, $error_ty:ty) => {
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
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __p2p_message_command_compression_impl {
    ($vis:vis $name:ident) => {
        impl $name {
            /// Returns true when C# Neo permits attempting LZ4 compression for this command.
            $vis const fn allows_compression(self) -> bool {
                matches!(
                    self,
                    Self::Block
                        | Self::Extensible
                        | Self::Transaction
                        | Self::Headers
                        | Self::Addr
                        | Self::MerkleBlock
                        | Self::FilterLoad
                        | Self::FilterAdd
                )
            }
        }
    };
}

/// Generates Neo P2P `MessageCommand` definitions from the shared command table.
#[macro_export]
macro_rules! p2p_message_command {
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            error = $error_ty:ty;
            parse_error = $parse_error:expr_2021;
            from_byte = result;
            extended_aliases = true;
        }
    ) => {
        $crate::__p2p_message_command_enum! {
            $(#[$enum_meta])*
            $vis $name
        }

        impl $name {
            /// Creates a command value from its byte representation.
            pub fn from_byte(byte: u8) -> ::std::result::Result<Self, $error_ty> {
                Ok(Self::from_byte_unchecked(byte))
            }

            /// Parses a command from its textual form.
            pub fn parse_str(s: &str) -> ::std::result::Result<Self, $error_ty> {
                $crate::__p2p_message_command_parse!($name, s, $parse_error, true)
            }
        }

        $crate::__p2p_message_command_shared_impls!($name, $error_ty);

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
            parse_error = $parse_error:expr_2021;
            from_byte = infallible;
            extended_aliases = false;
        }
    ) => {
        $crate::__p2p_message_command_enum! {
            $(#[$enum_meta])*
            $vis $name
        }

        impl $name {
            /// Creates a command value from its byte representation.
            pub const fn from_byte(byte: u8) -> Self {
                Self::from_byte_unchecked(byte)
            }

            /// Parses a command from its textual form.
            pub fn parse_str(s: &str) -> ::std::result::Result<Self, $error_ty> {
                $crate::__p2p_message_command_parse!($name, s, $parse_error, false)
            }
        }

        $crate::__p2p_message_command_shared_impls!($name, $error_ty);

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
