//! Serializable trait and helpers mirroring C# Neo.IO serialization contracts.

use crate::{IoResult, MemoryReader, binary_writer::BinaryWriter};

pub mod helper;
pub mod primitives;

/// Trait implemented by Neo types that can be serialized and deserialized.
///
/// This follows the behaviour of `Neo.IO.ISerializable` from the C# codebase.
pub trait Serializable: Sized {
    /// Creates an instance from the provided `MemoryReader`.
    fn deserialize(reader: &mut MemoryReader) -> IoResult<Self>;

    /// Serializes the current value into the provided `BinaryWriter`.
    fn serialize(&self, writer: &mut BinaryWriter) -> IoResult<()>;

    /// Returns the number of bytes the serialized value will consume.
    fn size(&self) -> usize;
}

/// Generates a complete `Serializable` implementation for a struct.
///
/// Supports primitive fields (`u8`, `i16`, `u16`, `i32`, `u32`, `i64`, `u64`, `bool`),
/// types implementing `Serializable`, and variable-length fields
/// (`var_bytes`, `var_string`, `var_array<T>`).
///
/// An optional `validate { ... }` block runs post-deserialization checks using `self_ref`.
///
/// # Example
///
/// ```ignore
/// impl_serializable! {
///     struct MyPayload {
///         index: u32,
///         hash: UInt256,
///         data: var_bytes { max: 520 },
///         items: var_array<Item> { max: 200 },
///     }
///     validate(self_ref) {
///         if self_ref.index == 0 {
///             return Err(IoError::invalid_data("index must be non-zero"));
///         }
///     }
/// }
/// ```
#[macro_export]
macro_rules! impl_serializable {
    (
        struct $name:ident {
            $( $field:ident : $first:ident $( < $inner:ty > )? $( { $($rest:tt)* } )? ),* $(,)?
        }
        $(validate ( $self_name:ident ) $validate_body:block)?
    ) => {
        impl $crate::Serializable for $name {
            fn size(&self) -> usize {
                let mut total = 0usize;
                $(
                    {
                        let __v = &self.$field;
                        $crate::impl_serializable!(@size total, __v, $first $( < $inner > )? $( { $($rest)* } )?);
                    }
                )*
                total
            }

            fn serialize(&self, writer: &mut $crate::BinaryWriter) -> $crate::IoResult<()> {
                $(
                    {
                        let __v = &self.$field;
                        $crate::impl_serializable!(@serialize writer, __v, $first $( < $inner > )? $( { $($rest)* } )?);
                    }
                )*
                Ok(())
            }

            fn deserialize(reader: &mut $crate::MemoryReader) -> $crate::IoResult<Self> {
                $(
                    $crate::impl_serializable!(@deserialize reader, $field, $first $( < $inner > )? $( { $($rest)* } )?);
                )*
                $crate::impl_serializable!(@make_result Self { $( $field, )* } $(, ($self_name) $validate_body)?)
            }
        }
    };

    // ── var_bytes ──────────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, var_bytes { max: $max:expr }) => {
        $total += $crate::serializable::helper::SerializeHelper::get_var_size_bytes($val);
    };
    (@serialize $writer:ident, $val:ident, var_bytes { max: $max:expr }) => {
        $writer.write_var_bytes($val)?;
    };
    (@deserialize $reader:ident, $field:ident, var_bytes { max: $max:expr }) => {
        let $field = $reader.read_var_bytes($max)?;
    };

    // ── var_string ─────────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, var_string { max: $max:expr }) => {
        $total += $crate::serializable::helper::SerializeHelper::get_var_size_str($val);
    };
    (@serialize $writer:ident, $val:ident, var_string { max: $max:expr }) => {
        $writer.write_var_string($val)?;
    };
    (@deserialize $reader:ident, $field:ident, var_string { max: $max:expr }) => {
        let $field = $reader.read_var_string($max)?;
    };

    // ── var_array<T> ───────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, var_array <$elem:ty> { max: $max:expr }) => {
        $total += $crate::serializable::helper::SerializeHelper::get_var_size_serializable_slice($val);
    };
    (@serialize $writer:ident, $val:ident, var_array <$elem:ty> { max: $max:expr }) => {
        $crate::serializable::helper::SerializeHelper::serialize_array($val, $writer)?;
    };
    (@deserialize $reader:ident, $field:ident, var_array <$elem:ty> { max: $max:expr }) => {
        let $field = $crate::serializable::helper::SerializeHelper::deserialize_array::<$elem>($reader, $max)?;
    };

    // ── primitive: u8 ──────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, u8) => { $total += 1; };
    (@serialize $writer:ident, $val:ident, u8) => { $writer.write_u8(*$val)?; };
    (@deserialize $reader:ident, $field:ident, u8) => { let $field = $reader.read_u8()?; };

    // ── primitive: i16 ─────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, i16) => { $total += 2; };
    (@serialize $writer:ident, $val:ident, i16) => { $writer.write_i16(*$val)?; };
    (@deserialize $reader:ident, $field:ident, i16) => { let $field = $reader.read_i16()?; };

    // ── primitive: u16 ─────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, u16) => { $total += 2; };
    (@serialize $writer:ident, $val:ident, u16) => { $writer.write_u16(*$val)?; };
    (@deserialize $reader:ident, $field:ident, u16) => { let $field = $reader.read_u16()?; };

    // ── primitive: i32 ─────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, i32) => { $total += 4; };
    (@serialize $writer:ident, $val:ident, i32) => { $writer.write_i32(*$val)?; };
    (@deserialize $reader:ident, $field:ident, i32) => { let $field = $reader.read_i32()?; };

    // ── primitive: u32 ─────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, u32) => { $total += 4; };
    (@serialize $writer:ident, $val:ident, u32) => { $writer.write_u32(*$val)?; };
    (@deserialize $reader:ident, $field:ident, u32) => { let $field = $reader.read_u32()?; };

    // ── primitive: i64 ─────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, i64) => { $total += 8; };
    (@serialize $writer:ident, $val:ident, i64) => { $writer.write_i64(*$val)?; };
    (@deserialize $reader:ident, $field:ident, i64) => { let $field = $reader.read_i64()?; };

    // ── primitive: u64 ─────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, u64) => { $total += 8; };
    (@serialize $writer:ident, $val:ident, u64) => { $writer.write_u64(*$val)?; };
    (@deserialize $reader:ident, $field:ident, u64) => { let $field = $reader.read_u64()?; };

    // ── primitive: bool ────────────────────────────────────────────────────
    (@size $total:ident, $val:ident, bool) => { $total += 1; };
    (@serialize $writer:ident, $val:ident, bool) => { $writer.write_bool(*$val)?; };
    (@deserialize $reader:ident, $field:ident, bool) => { let $field = $reader.read_bool()?; };

    // ── Serializable (fallback) ────────────────────────────────────────────
    (@size $total:ident, $val:ident, $field_ty:ident) => {
        $total += $crate::Serializable::size($val);
    };
    (@serialize $writer:ident, $val:ident, $field_ty:ident) => {
        $crate::Serializable::serialize($val, $writer)?;
    };
    (@deserialize $reader:ident, $field:ident, $field_ty:ident) => {
        let $field = <$field_ty as $crate::Serializable>::deserialize($reader)?;
    };

    // ── result construction with validate ──────────────────────────────────
    (@make_result $result:expr, ($self_name:ident) $validate_body:block) => {{
        let $self_name = $result;
        $validate_body
        Ok($self_name)
    }};

    // ── result construction without validate ───────────────────────────────
    (@make_result $result:expr) => {{
        Ok($result)
    }};
}
