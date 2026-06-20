/// Generates the Neo P2P message flag wrapper while preserving unknown bits.
///
/// Neo treats message flags as a raw byte with bit flags. Only bit `0x01`
/// currently means "compressed"; all other bits must round-trip for forward
/// compatibility. The generated wrapper is backed by `bitflags`, but keeps the
/// Neo byte-oriented API used by the network serializers.
#[macro_export]
macro_rules! protocol_message_flags {
    (
        $(#[$meta:meta])*
        $vis:vis $name:ident {
            warn_target = $warn_target:literal;
            from_byte = $from_byte:ident;
        }
    ) => {
        $crate::bitflags::bitflags! {
            $(#[$meta])*
            $vis struct $name: u8 {
                /// No flags are set.
                const NONE = 0x00;
                /// The payload is compressed.
                const COMPRESSED = 0x01;
                /// Preserve unknown flag bits for forward-compatible round-trips.
                const _ = !0;
            }
        }

        impl $name {
            /// Creates a new flag set with the given raw value.
            #[must_use]
            pub const fn new(value: u8) -> Self {
                Self::from_bits_retain(value)
            }

            /// Converts the flags to their byte representation.
            #[must_use]
            #[inline]
            pub const fn to_byte(self) -> u8 {
                self.bits()
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
                if byte & !Self::COMPRESSED.bits() != 0 {
                    ::tracing::warn!(
                        target: $warn_target,
                        "message flags include unknown bits (0x{:02x}); preserving raw value",
                        byte
                    );
                }
                Self::from_bits_retain(byte)
            }

            /// Returns `true` when the compressed flag is set.
            #[must_use]
            #[inline]
            pub const fn is_compressed(self) -> bool {
                self.bits() & Self::COMPRESSED.bits() != 0
            }

            /// Sets the compressed flag.
            pub fn set_compressed(&mut self, compressed: bool) {
                if compressed {
                    self.insert(Self::COMPRESSED);
                } else {
                    self.remove(Self::COMPRESSED);
                }
            }

            /// Returns a new flag set with the compressed flag updated.
            #[must_use]
            pub fn with_compressed(mut self, compressed: bool) -> Self {
                self.set_compressed(compressed);
                self
            }
        }

        impl ::std::fmt::Display for $name {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                let raw = self.to_byte();
                if raw == 0 {
                    write!(f, "None")
                } else if self.is_compressed() {
                    write!(f, "Compressed")
                } else {
                    write!(f, "Flags(0x{raw:02x})")
                }
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
                Ok(Self::from_bits_retain(value))
            }
        }
    };
}
