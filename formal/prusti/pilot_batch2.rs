//! Prusti pilot batch 2 — pure price/tag functions (conservative specs).
//! Compile with prusti-rustc.

use prusti_contracts::*;

const TAG_BOOLEAN: u8 = 0x01;
const TAG_INTEGER: u8 = 0x21;
const TAG_BYTESTRING: u8 = 0x28;
const TAG_BUFFER: u8 = 0x30;
const TAG_ARRAY: u8 = 0x40;
const TAG_STRUCT: u8 = 0x41;
const TAG_MAP: u8 = 0x48;
const TAG_POINTER: u8 = 0x60;
const TAG_INTEROP: u8 = 0x68;

/// Map raw StackItemType tag to compact runtime tag; unknown tags passthrough.
/// Written as an if-chain because Prusti 0.2.2 pure functions reject `match`.
#[pure]
pub fn normalize_stack_item_type_tag(type_tag: u8) -> u8 {
    if type_tag == TAG_BOOLEAN {
        type_tag
    } else if type_tag == TAG_INTEGER {
        type_tag
    } else if type_tag == TAG_BYTESTRING {
        type_tag
    } else if type_tag == TAG_BUFFER {
        type_tag
    } else if type_tag == TAG_ARRAY {
        type_tag
    } else if type_tag == TAG_STRUCT {
        type_tag
    } else if type_tag == TAG_MAP {
        type_tag
    } else if type_tag == TAG_POINTER {
        type_tag
    } else if type_tag == TAG_INTEROP {
        type_tag
    } else {
        type_tag
    }
}

/// Integer tag is already compact (identity).
#[pure]
#[ensures(normalize_stack_item_type_tag(TAG_INTEGER) == TAG_INTEGER)]
pub fn integer_tag_compact() -> u8 {
    normalize_stack_item_type_tag(TAG_INTEGER)
}

/// Array tag is already compact (identity).
#[pure]
#[ensures(normalize_stack_item_type_tag(TAG_ARRAY) == TAG_ARRAY)]
pub fn array_tag_compact() -> u8 {
    normalize_stack_item_type_tag(TAG_ARRAY)
}

/// ByteString tag is already compact.
#[pure]
#[ensures(normalize_stack_item_type_tag(TAG_BYTESTRING) == TAG_BYTESTRING)]
pub fn bytestring_tag_compact() -> u8 {
    normalize_stack_item_type_tag(TAG_BYTESTRING)
}

/// An unrecognized tag (outside the known set) passes through unchanged.
#[pure]
#[ensures(normalize_stack_item_type_tag(0xFE) == 0xFE)]
pub fn unknown_tag_passthrough() -> u8 {
    normalize_stack_item_type_tag(0xFE)
}

/// Integer-point price: PUSH0 costs 1.
#[pure]
#[ensures(result == 1)]
pub fn push0_price() -> i64 {
    let p: i64 = 0;
    if p == 0 { 1 } else { p }
}

/// PUSHDATA1 costs 8.
#[pure]
#[ensures(result == 8)]
pub fn pushdata1_price() -> i64 {
    8
}

/// APPEND costs 8192.
#[pure]
#[ensures(result == 8192)]
pub fn append_price() -> i64 {
    8192
}

/// Storage-class syscall costs 32768 (1<<15).
#[pure]
#[ensures(result == 32768)]
pub fn storage_syscall_price() -> i64 {
    32768
}

/// Runtime metadata syscall costs 8 (1<<3).
#[pure]
#[ensures(result == 8)]
pub fn runtime_metadata_price() -> i64 {
    8
}

/// CheckWitness costs 1024 (1<<10).
#[pure]
#[ensures(result == 1024)]
pub fn check_witness_price() -> i64 {
    1024
}

fn main() {}