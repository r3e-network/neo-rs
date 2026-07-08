/// Generates a fixed-width unsigned integer type with common boilerplate.
///
/// Generates: struct, new/zero/is_zero, byte conversions, Display/Debug/FromStr,
/// Ord (big-endian), infallible `from_array`, `From<[u8; N]>`, `TryFrom<&[u8]>`, and
/// `TryFrom<String>`.
/// Optional: AsRef (set `as_ref = true` when struct size == byte size, i.e. no padding).
#[macro_export]
macro_rules! uint_type {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            size = $size:expr_2021;
            size_const = $size_const:ident;
            $(#[$zero_meta:meta])*
            $zero_name:ident;
            as_ref = $as_ref:tt;
            fields: [$($field:ident : $fty:ty),+ $(,)?];
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(pub(crate) $field: $fty,)+
        }

        const _: () = assert!(0usize $(+ std::mem::size_of::<$fty>())+ == $size);

        $(#[$zero_meta])*
        $vis static $zero_name: $name = $name { $($field: 0),+ };

        impl $name {
            /// Byte length of this uint type.
            pub const LENGTH: usize = $size;

            #[inline]
            #[must_use]
            /// Returns the default zero value.
            pub fn new() -> Self { Self::default() }

            #[inline]
            #[must_use]
            /// Returns the all-zero value.
            pub const fn zero() -> Self { Self { $($field: 0),+ } }

            #[inline]
            #[must_use]
            /// Returns whether every byte of the value is zero.
            pub const fn is_zero(&self) -> bool { $(self.$field == 0)&&+ }

            /// Returns `true` when `other` is `Some` and equal to `self`.
            ///
            /// Mirrors C# `IEquatable<T>.Equals` with nullable comparison
            /// semantics (a value never equals the absence of one). Prefer the
            /// `==` operator in idiomatic Rust; this exists for C# parity.
            #[inline]
            #[must_use]
            pub fn equals(&self, other: Option<&Self>) -> bool {
                matches!(other, Some(o) if o == self)
            }

            #[inline]
            #[must_use]
            /// Returns the little-endian fixed-width byte array.
            pub fn as_bytes(&self) -> [u8; $size] { self.to_array() }

            #[inline]
            #[must_use]
            /// Returns the little-endian fixed-width bytes as a vector.
            pub fn to_bytes(&self) -> Vec<u8> {
                let mut bytes = Vec::with_capacity($size);
                $(bytes.extend_from_slice(&self.$field.to_le_bytes());)+
                bytes
            }

            #[inline]
            #[must_use]
            /// Creates the value from little-endian fixed-width bytes.
            pub fn from_array(data: [u8; $size]) -> Self {
                let mut result = Self::new();
                let mut offset = 0usize;
                $(
                    {
                        const FIELD_SIZE: usize = std::mem::size_of::<$fty>();
                        let mut buf = [0u8; FIELD_SIZE];
                        buf.copy_from_slice(&data[offset..offset + FIELD_SIZE]);
                        result.$field = <$fty>::from_le_bytes(buf);
                        offset += FIELD_SIZE;
                    }
                )+
                let _ = offset;
                result
            }

            #[inline]
            /// Parses the value from its little-endian fixed-width bytes.
            pub fn from_bytes(value: &[u8]) -> $crate::PrimitiveResult<Self> {
                if value.len() != $size {
                    return Err($crate::PrimitiveError::InvalidFormat {
                        message: format!("Invalid length: {}", value.len()),
                    });
                }
                let mut data = [0u8; $size];
                data.copy_from_slice(value);
                Ok(Self::from_array(data))
            }

            /// Parses the value from a byte span.
            pub fn try_from_span(value: &[u8]) -> $crate::PrimitiveResult<Self> {
                Self::from_bytes(value)
            }

            #[inline]
            #[must_use]
            /// Returns the little-endian fixed-width byte array.
            pub fn to_array(&self) -> [u8; $size] {
                let mut result = [0u8; $size];
                let mut offset = 0usize;
                $(
                    {
                        let bytes = self.$field.to_le_bytes();
                        result[offset..offset + bytes.len()].copy_from_slice(&bytes);
                        offset += bytes.len();
                    }
                )+
                let _ = offset;
                result
            }

            #[inline]
            #[must_use]
            /// Returns the little-endian fixed-width byte array.
            pub fn get_span(&self) -> [u8; $size] { self.to_array() }

            #[inline]
            /// Parses the reversed-hex string representation used by Neo.
            pub fn parse(s: &str) -> $crate::PrimitiveResult<Self> {
                let mut result = None;
                if !Self::try_parse(s, &mut result) {
                    return Err($crate::PrimitiveError::InvalidFormat {
                        message: "Invalid format".to_string(),
                    });
                }
                match result {
                    Some(value) => Ok(value),
                    None => Err($crate::PrimitiveError::InvalidFormat {
                        message: format!("Failed to parse {}", stringify!($name)),
                    }),
                }
            }

            /// Attempts to parse the reversed-hex string representation used by Neo.
            pub fn try_parse(s: &str, result: &mut Option<Self>) -> bool {
                match $crate::uint_hex::parse_reversed_hex::<$size>(s)
                    .and_then(|bytes| Self::from_bytes(&bytes))
                {
                    Ok(uint) => { *result = Some(uint); true }
                    Err(_) => false,
                }
            }

            #[must_use]
            /// Formats the value as Neo's reversed-hex string representation.
            pub fn to_hex_string(&self) -> String {
                $crate::uint_hex::format_reversed_hex(self.to_array())
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.to_hex_string())
            }
        }

        impl std::fmt::Debug for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}({})", stringify!($name), self.to_hex_string())
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::PrimitiveError;
            fn from_str(s: &str) -> Result<Self, Self::Err> { Self::parse(s) }
        }

        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $name {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                $crate::__uint_type_cmp!(@reverse self, other; [$($field),+])
            }
        }

        impl From<[u8; $size]> for $name {
            fn from(data: [u8; $size]) -> Self {
                Self::from_array(data)
            }
        }

        impl TryFrom<&[u8]> for $name {
            type Error = $crate::PrimitiveError;
            fn try_from(data: &[u8]) -> Result<Self, Self::Error> { Self::from_bytes(data) }
        }

        impl TryFrom<String> for $name {
            type Error = $crate::PrimitiveError;
            fn try_from(s: String) -> Result<Self, Self::Error> { Self::parse(&s) }
        }

        $crate::__uint_type_as_ref!($as_ref; $name, $size, $($field),+);
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __uint_type_as_ref {
    (true; $name:ident, $size:expr_2021, $($field:ident),+) => {
        impl AsRef<[u8; $size]> for $name {
            #[inline]
            // Rationale: generated fixed-width integer newtypes are layout-checked
            // below and expose zero-copy byte views on hot serialization paths.
            #[allow(unsafe_code)]
            fn as_ref(&self) -> &[u8; $size] {
                const _: () = assert!(
                    std::mem::size_of::<$name>() == $size,
                    concat!(stringify!($name), " has unexpected padding")
                );
                unsafe { &*(self as *const Self).cast::<[u8; $size]>() }
            }
        }
    };
    (false; $name:ident, $size:expr_2021, $($field:ident),+) => {};
}

#[doc(hidden)]
#[macro_export]
macro_rules! __uint_type_cmp {
    (@reverse $a:expr_2021, $b:expr_2021; [$first:ident $(, $rest:ident)*]) => {
        $crate::__uint_type_cmp!(@do_reverse $a, $b; [$first $(, $rest)*]; [])
    };
    (@do_reverse $a:expr_2021, $b:expr_2021; [$first:ident $(, $rest:ident)*]; [$($accum:ident),*]) => {
        $crate::__uint_type_cmp!(@do_reverse $a, $b; [$($rest),*]; [$first $(, $accum)*])
    };
    (@do_reverse $a:expr_2021, $b:expr_2021; []; [$($field:ident),+]) => {
        $crate::__uint_type_cmp!(@compare $a, $b; $($field),+)
    };
    (@compare $a:expr_2021, $b:expr_2021; $first:ident $(, $rest:ident)*) => {
        match $a.$first.cmp(&$b.$first) {
            std::cmp::Ordering::Equal => {
                $crate::__uint_type_cmp!(@compare $a, $b; $($rest),*)
            }
            other => other,
        }
    };
    (@compare $a:expr_2021, $b:expr_2021;) => {
        std::cmp::Ordering::Equal
    };
}
