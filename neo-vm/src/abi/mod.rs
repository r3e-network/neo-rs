//! Shared ABI and result types used at VM host/proof boundaries.

pub mod callback_codec;
mod cursor;
mod execution;
pub mod fast_codec;
pub mod result_codec;
mod stack_value;

pub use execution::{BackendKind, ExecutionResult, VmState};
pub use stack_value::{
    byte_sequence_bytes, byte_sequence_len, concat_splice_values, default_value_for_type_tag,
    encode_integer, new_array_default_value_for_neovm_type_tag,
    new_array_default_value_for_type_tag, normalize_stack_item_type_tag, pop_byte_arg,
    slice_splice_value, stack_value_as_bool, stack_value_as_bytes, stack_value_as_fixed_bytes,
    stack_value_as_i64, stack_value_as_string, stack_value_as_u32, stack_value_as_u8,
    stack_value_into_items, stack_value_span_bytes, StackItemType, StackValue, COMPACT_TAG_ARRAY,
    COMPACT_TAG_BIG_INTEGER, COMPACT_TAG_BOOLEAN, COMPACT_TAG_BUFFER, COMPACT_TAG_BYTESTRING,
    COMPACT_TAG_INTEGER, COMPACT_TAG_INTEROP, COMPACT_TAG_ITERATOR, COMPACT_TAG_MAP,
    COMPACT_TAG_NULL, COMPACT_TAG_POINTER, COMPACT_TAG_STRUCT, NEOVM_STACK_ITEM_TYPE_ANY,
    NEOVM_STACK_ITEM_TYPE_ARRAY, NEOVM_STACK_ITEM_TYPE_BOOLEAN, NEOVM_STACK_ITEM_TYPE_BUFFER,
    NEOVM_STACK_ITEM_TYPE_BYTESTRING, NEOVM_STACK_ITEM_TYPE_INTEGER,
    NEOVM_STACK_ITEM_TYPE_INTEROP_INTERFACE, NEOVM_STACK_ITEM_TYPE_MAP,
    NEOVM_STACK_ITEM_TYPE_POINTER, NEOVM_STACK_ITEM_TYPE_STRUCT, STACK_VALUE_CODEC_TAG_ARRAY,
    STACK_VALUE_CODEC_TAG_BIG_INTEGER, STACK_VALUE_CODEC_TAG_BOOLEAN, STACK_VALUE_CODEC_TAG_BUFFER,
    STACK_VALUE_CODEC_TAG_BYTESTRING, STACK_VALUE_CODEC_TAG_INTEGER, STACK_VALUE_CODEC_TAG_INTEROP,
    STACK_VALUE_CODEC_TAG_ITERATOR, STACK_VALUE_CODEC_TAG_MAP, STACK_VALUE_CODEC_TAG_NULL,
    STACK_VALUE_CODEC_TAG_POINTER, STACK_VALUE_CODEC_TAG_STRUCT,
};
