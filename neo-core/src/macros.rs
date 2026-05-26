//! Declarative macros for reducing boilerplate code.
//!
//! This module provides professional Rust macros that eliminate common
//! duplication patterns across the codebase, following DRY principles.

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
/// impl_from_bytes!(StorageItem, from_bytes_vec); // custom method name for Vec
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

/// Implements `From<T>` for enum variants that wrap a single value.
///
/// # Example
///
/// ```rust,ignore
/// enum_from! {
///     ConsensusMessagePayload,
///     ChangeView,
///     PrepareRequest,
///     PrepareResponse,
///     Commit,
/// }
/// ```
///
/// Expands to:
/// ```rust,ignore
/// impl From<ChangeView> for ConsensusMessagePayload {
///     fn from(value: ChangeView) -> Self {
///         ConsensusMessagePayload::ChangeView(value)
///     }
/// }
/// // ... for each variant
/// ```
#[macro_export]
macro_rules! enum_from {
    ($enum_type:ty, $($variant:ident),+ $(,)?) => {
        $(
            impl From<$variant> for $enum_type {
                fn from(value: $variant) -> Self {
                    <$enum_type>::$variant(value)
                }
            }
        )+
    };
}

/// Implements `From<T>` for enum variants with custom variant names.
///
/// # Example
///
/// ```rust,ignore
/// enum_from_named! {
///     MyEnum,
///     TypeA => VariantA,
///     TypeB => VariantB,
/// }
/// ```
#[macro_export]
macro_rules! enum_from_named {
    ($enum_type:ty, $($source:ty => $variant:ident),+ $(,)?) => {
        $(
            impl From<$source> for $enum_type {
                fn from(value: $source) -> Self {
                    <$enum_type>::$variant(value)
                }
            }
        )+
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

/// Creates a newtype wrapper with common trait implementations.
///
/// # Example
///
/// ```rust,ignore
/// newtype! {
///     /// Documentation for the type
///     pub struct MyId(u64);
/// }
/// ```
///
/// Generates: `Debug`, `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `From`, `Into`
#[macro_export]
macro_rules! newtype {
    ($(#[$meta:meta])* $vis:vis struct $name:ident($inner_vis:vis $inner:ty);) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        $vis struct $name($inner_vis $inner);

        impl From<$inner> for $name {
            fn from(value: $inner) -> Self {
                Self(value)
            }
        }

        impl From<$name> for $inner {
            fn from(value: $name) -> Self {
                value.0
            }
        }

        impl $name {
            /// Returns the inner value.
            #[inline]
            pub const fn inner(&self) -> $inner {
                self.0
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

/// Implements `Display` for an enum by matching variants to string representations.
///
/// # Example
///
/// ```rust,ignore
/// impl_display_enum! {
///     MyEnum,
///     Variant1 => "variant_1",
///     Variant2 => "variant_2",
/// }
/// ```
#[macro_export]
macro_rules! impl_display_enum {
    ($enum_type:ty, $($variant:ident => $display:expr_2021),+ $(,)?) => {
        impl std::fmt::Display for $enum_type {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                match self {
                    $(
                        Self::$variant => write!(f, $display),
                    )+
                }
            }
        }
    };
}

/// Implements `TryFrom<u8>` for an enum with explicit discriminants.
///
/// # Example
///
/// ```rust,ignore
/// impl_try_from_u8! {
///     MyEnum,
///     0 => Variant1,
///     1 => Variant2,
///     2 => Variant3,
/// }
/// ```
#[macro_export]
macro_rules! impl_try_from_u8 {
    ($enum_type:ty, $($value:expr_2021 => $variant:ident),+ $(,)?) => {
        impl TryFrom<u8> for $enum_type {
            type Error = $crate::CoreError;

            fn try_from(value: u8) -> Result<Self, Self::Error> {
                match value {
                    $(
                        $value => Ok(Self::$variant),
                    )+
                    _ => Err($crate::CoreError::invalid_data(
                        format!("invalid {} value: {}", stringify!($enum_type), value)
                    )),
                }
            }
        }
    };
}

/// Implements serialization helpers for types with `to_bytes()` and `from_bytes()`.
///
/// # Example
///
/// ```rust,ignore
/// impl_serializable_bytes!(UInt160, 20);
/// impl_serializable_bytes!(UInt256, 32);
/// ```
#[macro_export]
macro_rules! impl_serializable_bytes {
    ($type:ty, $size:expr_2021) => {
        impl $crate::neo_io::Serializable for $type {
            fn serialize(
                &self,
                writer: &mut $crate::neo_io::BinaryWriter,
            ) -> $crate::neo_io::IoResult<()> {
                writer.write_bytes(&self.to_bytes())
            }

            fn deserialize(
                reader: &mut $crate::neo_io::MemoryReader<'_>,
            ) -> $crate::neo_io::IoResult<Self> {
                let bytes = reader.read_bytes($size)?;
                Ok(Self::from_bytes(&bytes))
            }
        }
    };
}

/// Generates getter methods for struct fields.
///
/// # Example
///
/// ```rust,ignore
/// impl MyStruct {
///     getters! {
///         name: String,
///         age: u32,
///         active: bool,
///     }
/// }
/// ```
#[macro_export]
macro_rules! getters {
    ($($field:ident: $type:ty),+ $(,)?) => {
        $(
            #[inline]
            pub fn $field(&self) -> &$type {
                &self.$field
            }
        )+
    };
}

/// Generates getter methods that return copies for `Copy` types.
///
/// # Example
///
/// ```rust,ignore
/// impl MyStruct {
///     copy_getters! {
///         id: u64,
///         count: usize,
///     }
/// }
/// ```
#[macro_export]
macro_rules! copy_getters {
    ($($field:ident: $type:ty),+ $(,)?) => {
        $(
            #[inline]
            pub fn $field(&self) -> $type {
                self.$field
            }
        )+
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

/// Extension trait for `Option<T>` to simplify error handling with `IoError`.
///
/// # Example
///
/// ```rust,ignore
/// use neo_core::macros::OptionExt;
///
/// let value: Option<u32> = Some(42);
/// let result = value.ok_or_invalid_data("Value not found")?;
/// ```
pub trait OptionExt<T> {
    /// Converts `Option<T>` to `IoResult<T>` with an invalid data error message.
    fn ok_or_invalid_data(self, msg: &str) -> crate::neo_io::IoResult<T>;
}

impl<T> OptionExt<T> for Option<T> {
    fn ok_or_invalid_data(self, msg: &str) -> crate::neo_io::IoResult<T> {
        self.ok_or_else(|| crate::neo_io::IoError::invalid_data(msg))
    }
}

/// Extension trait for validating collection lengths.
///
/// # Example
///
/// ```rust,ignore
/// use neo_core::macros::ValidateLength;
///
/// let data = vec![1, 2, 3];
/// data.validate_max_length(10, "data")?;
/// ```
pub trait ValidateLength {
    /// Validates that the length does not exceed the maximum.
    fn validate_max_length(&self, max: usize, field_name: &str) -> crate::neo_io::IoResult<()>;
}

impl<T> ValidateLength for Vec<T> {
    fn validate_max_length(&self, max: usize, field_name: &str) -> crate::neo_io::IoResult<()> {
        if self.len() > max {
            Err(crate::neo_io::IoError::invalid_data(format!(
                "{} exceeds maximum length of {} (got {})",
                field_name,
                max,
                self.len()
            )))
        } else {
            Ok(())
        }
    }
}

impl ValidateLength for [u8] {
    fn validate_max_length(&self, max: usize, field_name: &str) -> crate::neo_io::IoResult<()> {
        if self.len() > max {
            Err(crate::neo_io::IoError::invalid_data(format!(
                "{} exceeds maximum length of {} (got {})",
                field_name,
                max,
                self.len()
            )))
        } else {
            Ok(())
        }
    }
}

impl ValidateLength for str {
    fn validate_max_length(&self, max: usize, field_name: &str) -> crate::neo_io::IoResult<()> {
        if self.len() > max {
            Err(crate::neo_io::IoError::invalid_data(format!(
                "{} exceeds maximum length of {} (got {})",
                field_name,
                max,
                self.len()
            )))
        } else {
            Ok(())
        }
    }
}

impl ValidateLength for String {
    fn validate_max_length(&self, max: usize, field_name: &str) -> crate::neo_io::IoResult<()> {
        self.as_str().validate_max_length(max, field_name)
    }
}

/// Macro to validate and write variable-length bytes with a maximum size check.
///
/// # Example
///
/// ```rust,ignore
/// validate_and_write_var_bytes!(writer, self.data, MAX_DATA_SIZE, "data");
/// ```
#[macro_export]
macro_rules! validate_and_write_var_bytes {
    ($writer:expr_2021, $data:expr_2021, $max:expr_2021, $name:expr_2021) => {{
        use $crate::macros::ValidateLength;
        $data.validate_max_length($max, $name)?;
        $writer.write_var_bytes(&$data)?;
    }};
}

/// Macro to implement multiple `impl Default` via `new()` at once.
///
/// # Example
///
/// ```rust,ignore
/// impl_default_via_new_batch!(Header, Block, Transaction);
/// ```
#[macro_export]
macro_rules! impl_default_via_new_batch {
    ($($type:ty),+ $(,)?) => {
        $(
            impl Default for $type {
                fn default() -> Self {
                    Self::new()
                }
            }
        )+
    };
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_newtype_macro() {
        newtype! {
            /// Test ID type
            pub struct TestId(pub u64);
        }

        let id = TestId::from(42u64);
        assert_eq!(id.inner(), 42);
        assert_eq!(u64::from(id), 42);
    }

    #[test]
    fn test_impl_ord_by_fields() {
        #[derive(Debug, Clone, PartialEq, Eq)]
        struct Point {
            x: i32,
            y: i32,
        }

        impl_ord_by_fields!(Point, x, y);

        let p1 = Point { x: 1, y: 2 };
        let p2 = Point { x: 1, y: 3 };
        let p3 = Point { x: 2, y: 1 };

        assert!(p1 < p2);
        assert!(p2 < p3);
        assert!(p1 < p3);
    }

    #[test]
    fn test_enum_from_macro() {
        #[derive(Debug, PartialEq)]
        struct TypeA(u32);

        #[derive(Debug, PartialEq)]
        struct TypeB(String);

        #[derive(Debug, PartialEq)]
        enum MyEnum {
            TypeA(TypeA),
            TypeB(TypeB),
        }

        enum_from!(MyEnum, TypeA, TypeB);

        let a = TypeA(42);
        let b = TypeB("hello".to_string());

        let enum_a: MyEnum = a.into();
        let enum_b: MyEnum = b.into();

        assert!(matches!(enum_a, MyEnum::TypeA(TypeA(42))));
        assert!(matches!(enum_b, MyEnum::TypeB(_)));
    }
}
