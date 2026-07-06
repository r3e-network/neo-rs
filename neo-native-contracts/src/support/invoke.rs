//! Native method binding helpers.
//!
//! The execution engine validates ABI metadata, call flags, hardfork gates, and
//! fees before calling a native contract. This module keeps the final
//! ABI-name-to-Rust-handler binding explicit while allowing a contract to derive
//! its manifest method table from the same binding list.

use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, NativeMethod};

/// Rust implementation function for one native ABI method.
pub(crate) type NativeMethodHandler<C> =
    fn(&C, &mut ApplicationEngine, &[Vec<u8>]) -> CoreResult<Vec<u8>>;

/// Pairs one manifest method descriptor with the Rust handler that implements it.
pub(crate) struct NativeMethodBinding<C> {
    method: NativeMethod,
    handler: NativeMethodHandler<C>,
}

impl<C> NativeMethodBinding<C> {
    /// Creates a native method binding from its ABI descriptor and handler.
    pub(crate) const fn new(method: NativeMethod, handler: NativeMethodHandler<C>) -> Self {
        Self { method, handler }
    }

    /// Returns the ABI method descriptor.
    pub(crate) fn method(&self) -> &NativeMethod {
        &self.method
    }

    fn matches_name(&self, method: &str) -> bool {
        self.method.name == method
    }

    fn invoke(
        &self,
        contract: &C,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        (self.handler)(contract, engine, args)
    }
}

/// Clones a manifest method table from the contract's binding table.
pub(crate) fn method_metadata<C>(bindings: &[NativeMethodBinding<C>]) -> Vec<NativeMethod> {
    bindings
        .iter()
        .map(|binding| binding.method().clone())
        .collect()
}

/// Dispatches a non-overloaded native method by ABI name.
///
/// `ApplicationEngine::call_native_contract` has already resolved the ABI
/// descriptor by `(name, arity, hardfork era)` before invoking the contract.
/// Direct unit tests can still call `NativeContract::invoke` directly, so this
/// helper intentionally preserves the historical per-contract name dispatch
/// behavior instead of adding a second arity gate here.
pub(crate) fn dispatch_by_name<C>(
    contract: &C,
    bindings: &[NativeMethodBinding<C>],
    engine: &mut ApplicationEngine,
    method: &str,
    args: &[Vec<u8>],
) -> Option<CoreResult<Vec<u8>>> {
    bindings
        .iter()
        .find(|binding| binding.matches_name(method))
        .map(|binding| binding.invoke(contract, engine, args))
}
