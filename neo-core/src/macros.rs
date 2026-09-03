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

#[cfg(test)]
mod tests {
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
}
