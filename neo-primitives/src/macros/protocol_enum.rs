#[doc(hidden)]
#[macro_export]
macro_rules! __protocol_enum_display {
    ($variant:ident) => {
        stringify!($variant)
    };
    ($variant:ident, $display:expr_2021) => {
        $display
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __protocol_enum_count {
    ($($variant:ident),+ $(,)?) => {
        <[()]>::len(&[$($crate::__protocol_enum_count!(@unit $variant)),+])
    };
    (@unit $variant:ident) => {
        ()
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
///
/// Add the `all;` prefix when callers need a generated declaration-order table:
///
/// ```rust,ignore
/// protocol_enum! {
///     all;
///     #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
///     pub MessageType {
///         Vote = 0,
///         StateRoot = 1,
///     }
/// }
/// ```
#[macro_export]
macro_rules! protocol_enum {
    (
        all;
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr_2021 $(=> $display:expr_2021)?
            ),+ $(,)?
        }
    ) => {
        $crate::protocol_enum! {
            $(#[$enum_meta])*
            $vis $name {
                $(
                    $(#[$variant_meta])*
                    $variant = $byte $(=> $display)?
                ),+
            }
        }

        impl $name {
            /// All known values in declaration order.
            pub const ALL: [Self; $crate::__protocol_enum_count!($($variant),+)] = [
                $(Self::$variant),+
            ];

            /// Number of known values.
            pub const COUNT: usize = Self::ALL.len();

            /// Returns all known values in declaration order.
            #[must_use]
            pub const fn all() -> [Self; $crate::__protocol_enum_count!($($variant),+)] {
                Self::ALL
            }

            /// Returns the number of known values.
            #[must_use]
            pub const fn count() -> usize {
                Self::COUNT
            }
        }
    };

    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr_2021 $(=> $display:expr_2021)?
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

/// Implements case-insensitive `FromStr` for enums generated by [`protocol_enum!`]
/// or [`protocol_enum_repr!`].
///
/// The generated parser reuses the enum's canonical `as_str()` table, so callers
/// do not need to maintain a second string-to-variant match.
#[macro_export]
macro_rules! impl_protocol_enum_from_str {
    (
        $name:ident {
            error = $error:expr_2021;
            aliases = [$($alias:literal => $alias_variant:ident),* $(,)?];
        }
    ) => {
        impl ::std::str::FromStr for $name {
            type Err = ::std::string::String;

            fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
                for candidate in Self::ALL {
                    if value.eq_ignore_ascii_case(candidate.as_str()) {
                        return Ok(candidate);
                    }
                }
                $(
                    if value.eq_ignore_ascii_case($alias) {
                        return Ok(Self::$alias_variant);
                    }
                )*
                Err(($error)(value))
            }
        }
    };

    (
        $name:ident {
            error = $error:expr_2021;
        }
    ) => {
        impl ::std::str::FromStr for $name {
            type Err = ::std::string::String;

            fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
                for candidate in Self::ALL {
                    if value.eq_ignore_ascii_case(candidate.as_str()) {
                        return Ok(candidate);
                    }
                }
                Err(($error)(value))
            }
        }
    };

    (
        $name:ident {
            error = $error:expr_2021;
            aliases = [$($alias:literal => $alias_variant:ident),* $(,)?];
            $($variant:ident),+ $(,)?
        }
    ) => {
        impl ::std::str::FromStr for $name {
            type Err = ::std::string::String;

            fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
                $(
                    if value.eq_ignore_ascii_case(Self::$variant.as_str()) {
                        return Ok(Self::$variant);
                    }
                )+
                $(
                    if value.eq_ignore_ascii_case($alias) {
                        return Ok(Self::$alias_variant);
                    }
                )*
                Err(($error)(value))
            }
        }
    };

    (
        $name:ident {
            error = $error:expr_2021;
            $($variant:ident),+ $(,)?
        }
    ) => {
        impl ::std::str::FromStr for $name {
            type Err = ::std::string::String;

            fn from_str(value: &str) -> ::std::result::Result<Self, Self::Err> {
                $(
                    if value.eq_ignore_ascii_case(Self::$variant.as_str()) {
                        return Ok(Self::$variant);
                    }
                )+
                Err(($error)(value))
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
        all;
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr_2021 $(=> $display:expr_2021)?
            ),+ $(,)?
        }
    ) => {
        $crate::protocol_enum_repr! {
            $(#[$enum_meta])*
            $vis $name {
                $(
                    $(#[$variant_meta])*
                    $variant = $byte $(=> $display)?
                ),+
            }
        }

        impl $name {
            /// All known values in declaration order.
            pub const ALL: [Self; $crate::__protocol_enum_count!($($variant),+)] = [
                $(Self::$variant),+
            ];

            /// Number of known values.
            pub const COUNT: usize = Self::ALL.len();

            /// Returns all known values in declaration order.
            #[must_use]
            pub const fn all() -> [Self; $crate::__protocol_enum_count!($($variant),+)] {
                Self::ALL
            }

            /// Returns the number of known values.
            #[must_use]
            pub const fn count() -> usize {
                Self::COUNT
            }
        }
    };

    (
        $(#[$enum_meta:meta])*
        $vis:vis $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr_2021 $(=> $display:expr_2021)?
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
            $unknown:ident(u8) => $unknown_display:expr_2021;

            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr_2021 $(=> $display:expr_2021)?
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
            $unknown:ident(u8) => $unknown_display:expr_2021;

            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $byte:expr_2021 $(=> $display:expr_2021)?
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
