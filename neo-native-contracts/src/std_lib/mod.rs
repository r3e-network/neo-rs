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

impl<P> NativeContract<P> for StdLib
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
{
    native_contract_identity!(StdLib);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::STD_LIB_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    native_contract_dispatch!(
        metadata::std_lib_method_bindings,
        by_name_and_arity,
        resolved_by_index = metadata::invoke_std_lib_method_by_index
    );

    fn try_invoke_resolved_stack_items<D, B>(
        &self,
        engine: &mut neo_execution::ApplicationEngine<P, D, B>,
        method_index: usize,
        _method: &neo_execution::NativeMethod,
        args: &[neo_vm::StackItem],
    ) -> Option<neo_error::CoreResult<Option<neo_vm::StackItem>>>
    where
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        metadata::invoke_std_lib_stack_item_method_by_index(self, engine, method_index, args)
    }
}

#[cfg(test)]
use neo_error::CoreResult;

#[cfg(test)]
mod test_dispatch;

#[cfg(test)]
#[path = "../tests/std_lib/mod.rs"]
mod tests;
