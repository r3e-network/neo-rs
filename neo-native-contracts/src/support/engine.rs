//! Shared `ApplicationEngine` prelude helpers for native contracts.
//!
//! Replaces the repeated `engine.persisting_block().ok_or_else(...)` prelude
//! found at the top of `on_persist` / `post_persist` methods across the native
//! contracts.

use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, Diagnostic};
use neo_payloads::Block;

/// Returns a reference to the persisting block, or an error if none is set.
///
/// Replaces the repeated pattern (found in 4 sites):
///
/// ```ignore
/// let block = engine.persisting_block().ok_or_else(|| {
///     CoreError::invalid_operation("CONTRACT::method requires a persisting block")
/// })?;
/// ```
///
/// The `contract` label is used in the error message for diagnostics, e.g.
/// `"Notary::on_persist"`, `"GasToken::on_persist"`.
pub(crate) fn require_persisting_block<'a, P, D, B>(
    engine: &'a ApplicationEngine<P, D, B>,
    contract: &str,
) -> CoreResult<&'a Block>
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    engine.persisting_block().ok_or_else(|| {
        CoreError::invalid_operation(format!("{contract} requires a persisting block"))
    })
}
