//! # neo-rpc::server::rpc_helpers
//!
//! Shared helper functions for RPC handler implementations.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `bytes`: Base64 and Neo wire-payload encoding helpers.
//! - `errors`: Shared RPC error constructors.
//! - `hashes`: UInt160/UInt256 and address parsing helpers.
//! - `params`: Generic positional parameter parsing helpers.
//! - `tests`: Module-local tests and regression coverage.

mod bytes;
mod errors;
mod hashes;
mod params;

pub use bytes::{
    decode_base64_text, decode_trimmed_base64_text, expect_base64_param,
    expect_base64_param_with_decode_message, expect_base64_param_with_message,
    expect_base64_param_with_messages, serialize_to_base64,
};
pub use errors::{internal_error, invalid_params};
pub use hashes::{
    expect_hash_param, expect_script_hash_or_address_param, expect_uint160_param_with_message,
    expect_uint256_param_with_message, parse_script_hash_or_address,
    parse_script_hash_or_address_with_error, parse_uint160, parse_uint160_text_with_label,
    parse_uint256, parse_uint256_text_with_label,
};
pub use params::{
    expect_string_param, expect_string_param_with_message, expect_u32_param,
    expect_u32_param_with_message, expect_u64_param, optional_u64_param, optional_usize_param,
    parse_verbose,
};

#[cfg(test)]
#[path = "../../tests/server/core/rpc_helpers.rs"]
mod tests;
