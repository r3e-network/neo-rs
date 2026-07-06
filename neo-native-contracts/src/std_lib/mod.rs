//! # neo-native-contracts::std_lib
//!
//! Native StdLib string, memory, and serialization helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `args`: argument extraction and C# max-input-length validation.
//! - `encoding`: encoding and decoding routines.
//! - `invoke`: native method handlers and hardfork-gated ABI entry points.
//! - `memory`: memory comparison/search helpers.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `numeric`: itoa/atoi and .NET integer-cast compatibility helpers.
//! - `serialization`: serialization codecs and compatibility checks.
//! - `strings`: stringSplit and strLen helpers.
//! - `test_dispatch`: test-only pure arity dispatch for compatibility vectors.
//! - `tests`: Module-local tests and regression coverage.

mod args;
mod encoding;
mod invoke;
mod memory;
mod metadata;
mod numeric;
mod serialization;
mod strings;

use neo_execution::{NativeContract, NativeMethod};

use crate::hashes::STDLIB_HASH;

native_contract_handle!(
    /// The StdLib native contract.
    pub struct StdLib {
        id: -2,
        contract_name: "StdLib",
        hash: STDLIB_HASH,
    }
);

impl NativeContract for StdLib {
    native_contract_identity!(StdLib);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::STD_LIB_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    native_contract_dispatch!(metadata::STD_LIB_METHOD_BINDINGS, by_name_and_arity);
}

#[cfg(test)]
use neo_error::CoreResult;

#[cfg(test)]
mod test_dispatch;

#[cfg(test)]
#[path = "../tests/std_lib/mod.rs"]
mod tests;
