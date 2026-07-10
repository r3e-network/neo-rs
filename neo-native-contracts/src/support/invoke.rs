//! Native method binding helpers.
//!
//! The execution engine validates ABI metadata, call flags, hardfork gates, and
//! fees before calling a native contract. This module keeps the final
//! ABI-name-to-Rust-handler binding explicit while allowing a contract to derive
//! its manifest method table from the same binding list.

use neo_error::CoreResult;
use neo_execution::native_contract_provider::{NativeContractProvider, NoNativeContractProvider};
use neo_execution::{ApplicationEngine, Diagnostic, NativeMethod, NoDiagnostic};
use neo_storage::{CacheRead, EmptyCacheBacking};

/// Rust implementation function for one native ABI method.
pub(crate) type NativeMethodHandler<
    C,
    P = NoNativeContractProvider,
    D = NoDiagnostic,
    B = EmptyCacheBacking,
> = fn(&C, &mut ApplicationEngine<P, D, B>, &[Vec<u8>]) -> CoreResult<Vec<u8>>;

/// Pairs one manifest method descriptor with the Rust handler that implements it.
pub(crate) struct NativeMethodBinding<
    C,
    P = NoNativeContractProvider,
    D = NoDiagnostic,
    B = EmptyCacheBacking,
> where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    method: NativeMethod,
    handler: NativeMethodHandler<C, P, D, B>,
}

impl<C, P, D, B> NativeMethodBinding<C, P, D, B>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    /// Creates a native method binding from its ABI descriptor and handler.
    pub(crate) const fn new(
        method: NativeMethod,
        handler: NativeMethodHandler<C, P, D, B>,
    ) -> Self {
        Self { method, handler }
    }

    /// Returns the ABI method descriptor.
    pub(crate) fn method(&self) -> &NativeMethod {
        &self.method
    }

    fn matches_name(&self, method: &str) -> bool {
        self.method.name == method
    }

    fn matches_name_and_arity(&self, method: &str, arity: usize) -> bool {
        self.matches_name(method) && self.method.parameters.len() == arity
    }

    fn invoke(
        &self,
        contract: &C,
        engine: &mut ApplicationEngine<P, D, B>,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        (self.handler)(contract, engine, args)
    }
}

/// Clones a manifest method table from the contract's binding table.
pub(crate) fn method_metadata<C, P, D, B>(
    bindings: &[NativeMethodBinding<C, P, D, B>],
) -> Vec<NativeMethod>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
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
pub(crate) fn dispatch_by_name<C, P, D, B>(
    contract: &C,
    bindings: &[NativeMethodBinding<C, P, D, B>],
    engine: &mut ApplicationEngine<P, D, B>,
    method: &str,
    args: &[Vec<u8>],
) -> Option<CoreResult<Vec<u8>>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    bindings
        .iter()
        .find(|binding| binding.matches_name(method))
        .map(|binding| binding.invoke(contract, engine, args))
}

/// Dispatches an overloaded native method by ABI name and argument count.
///
/// Use this for contracts such as StdLib where the Neo ABI exposes multiple
/// descriptors with the same name. Returning `None` for a wrong arity mirrors
/// the engine's metadata resolution instead of falling into the first same-name
/// handler.
pub(crate) fn dispatch_by_name_and_arity<C, P, D, B>(
    contract: &C,
    bindings: &[NativeMethodBinding<C, P, D, B>],
    engine: &mut ApplicationEngine<P, D, B>,
    method: &str,
    args: &[Vec<u8>],
) -> Option<CoreResult<Vec<u8>>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    bindings
        .iter()
        .find(|binding| binding.matches_name_and_arity(method, args.len()))
        .map(|binding| binding.invoke(contract, engine, args))
}

/// Dispatches a native method by the binding-table index already resolved by
/// the execution engine.
///
/// Standard native contracts build `methods()` with [`method_metadata`] from
/// the same binding table, preserving order. `ApplicationEngine` can therefore
/// resolve ABI metadata once, charge fees/check flags from that record, then
/// call this helper with the selected index instead of repeating string/arity
/// dispatch inside the native contract.
pub(crate) fn dispatch_by_index<C, P, D, B>(
    contract: &C,
    bindings: &[NativeMethodBinding<C, P, D, B>],
    engine: &mut ApplicationEngine<P, D, B>,
    method_index: usize,
    args: &[Vec<u8>],
) -> Option<CoreResult<Vec<u8>>>
where
    P: NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: CacheRead,
{
    bindings
        .get(method_index)
        .map(|binding| binding.invoke(contract, engine, args))
}
