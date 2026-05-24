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
#[macro_export]
macro_rules! protocol_enum {
    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr
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
            #[must_use]
            #[inline]
            pub const fn to_byte(self) -> u8 {
                self as u8
            }

            #[must_use]
            pub const fn from_byte(value: u8) -> Option<Self> {
                match value {
                    $(
                        $byte => Some(Self::$variant),
                    )+
                    _ => None,
                }
            }

            #[must_use]
            pub const fn as_str(self) -> &'static str {
                match self {
                    $(
                        Self::$variant => stringify!($variant),
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
                let byte = u8::deserialize(deserializer)?;
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
