/// Implements `Display` for a type by formatting its byte representation as hex.
///
/// The second argument is a field name whose value is a byte container
/// (`Vec<u8>`, `&[u8]`, etc.). Each byte is written as a two-digit
/// lowercase hex pair (e.g. `0a`).
///
/// # Example
///
/// ```rust,ignore
/// impl_display_hex!(MyType, data);
/// ```
#[macro_export]
macro_rules! impl_display_hex {
    ($type:ty, $field:ident) => {
        impl ::std::fmt::Display for $type {
            fn fmt(&self, f: &mut ::std::fmt::Formatter<'_>) -> ::std::fmt::Result {
                for byte in &self.$field {
                    ::std::write!(f, "{byte:02x}")?;
                }
                ::std::result::Result::Ok(())
            }
        }
    };
}

// === Macros migrated from neo-core ===

/// Implements `From<T>` for error types that convert via `.to_string()`.
///
/// # Example
///
/// ```rust,ignore
/// impl_error_from! {
///     CoreError,
///     std::io::Error => io,
///     std::fmt::Error => serialization,
/// }
/// ```
///
/// Expands to:
/// ```rust,ignore
/// impl From<std::io::Error> for CoreError {
///     fn from(error: std::io::Error) -> Self {
///         CoreError::io(error.to_string())
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_error_from {
    ($error_type:ty, $($source:ty => $method:ident),+ $(,)?) => {
        $(
            impl From<$source> for $error_type {
                fn from(error: $source) -> Self {
                    <$error_type>::$method(error.to_string())
                }
            }
        )+
    };
}

/// Implements `From<Vec<u8>>` and `From<&[u8]>` for types with a `from_bytes` method.
///
/// # Example
///
/// ```rust,ignore
/// impl_from_bytes!(StorageKey);
/// impl_from_bytes!(StorageItem, owned: from_bytes_vec);
/// ```
#[macro_export]
macro_rules! impl_from_bytes {
    ($type:ty) => {
        impl From<Vec<u8>> for $type {
            fn from(value: Vec<u8>) -> Self {
                Self::from_bytes(&value)
            }
        }

        impl From<&[u8]> for $type {
            fn from(value: &[u8]) -> Self {
                Self::from_bytes(value)
            }
        }
    };
    ($type:ty, owned: $owned_method:ident) => {
        impl From<Vec<u8>> for $type {
            fn from(value: Vec<u8>) -> Self {
                Self::$owned_method(value)
            }
        }

        impl From<&[u8]> for $type {
            fn from(value: &[u8]) -> Self {
                Self::$owned_method(value.to_vec())
            }
        }
    };
}

/// Implements ordering traits (`PartialOrd`, `Ord`) based on field comparison order.
///
/// # Example
///
/// ```rust,ignore
/// impl_ord_by_fields!(MyStruct, field1, field2, field3);
/// ```
#[macro_export]
macro_rules! impl_ord_by_fields {
    ($type:ty, $($field:ident),+ $(,)?) => {
        impl PartialOrd for $type {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }

        impl Ord for $type {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                $(
                    match self.$field.cmp(&other.$field) {
                        std::cmp::Ordering::Equal => {}
                        ord => return ord,
                    }
                )+
                std::cmp::Ordering::Equal
            }
        }
    };
}

/// Implements `Default` for a struct by calling `Self::new()`.
///
/// # Example
///
/// ```rust,ignore
/// impl_default_via_new!(MyStruct);
/// ```
#[macro_export]
macro_rules! impl_default_via_new {
    ($type:ty) => {
        impl Default for $type {
            fn default() -> Self {
                Self::new()
            }
        }
    };
}

/// Implements `Hash` for a struct based on specified fields.
///
/// # Example
///
/// ```rust,ignore
/// impl_hash_for_fields!(Signer, account, scopes);
/// ```
#[macro_export]
macro_rules! impl_hash_for_fields {
    ($type:ty, $($field:ident),+ $(,)?) => {
        impl std::hash::Hash for $type {
            fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
                $(self.$field.hash(state);)+
            }
        }
    };
}
