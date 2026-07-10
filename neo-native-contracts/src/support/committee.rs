//! Shared committee-witness verification for native contracts.
//!
//! Mirrors C# `NativeContract.AssertCommittee`: returns an error unless the
//! committee multisig address witnessed the call. Used by all committee-gated
//! setters across the native contracts (Policy, RoleManagement, Treasury,
//! Notary, Oracle, ContractManagement, NeoToken).

use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;

/// Returns whether the executing witness is the committee multisig address.
///
/// Shared by [`assert_committee`] (which turns `false` into an error) and by
/// `Treasury::verify` (C# `Treasury.Verify` = `CheckCommittee(engine)`), which
/// needs the boolean result directly. Centralizing the
/// [`ApplicationEngine::check_committee_witness`] call keeps the diagnostic
/// error wording identical across every committee-gated native method.
pub(crate) fn is_committee_witness<
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: neo_execution::Diagnostic + 'static,
    B: neo_storage::CacheRead,
>(
    engine: &ApplicationEngine<P, D, B>,
    method: &str,
) -> CoreResult<bool> {
    engine
        .check_committee_witness()
        .map_err(|e| CoreError::invalid_operation(format!("{method} committee check: {e}")))
}

/// Verifies that the executing witness is the committee multisig address.
///
/// On failure returns `CoreError::invalid_operation` with the supplied
/// `method` name for diagnostics. Equivalent to the C# static helper used by
/// every `OnlyCommittee` setter.
pub(crate) fn assert_committee<
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: neo_execution::Diagnostic + 'static,
    B: neo_storage::CacheRead,
>(
    engine: &ApplicationEngine<P, D, B>,
    method: &str,
) -> CoreResult<()> {
    if !is_committee_witness(engine, method)? {
        return Err(CoreError::invalid_operation(format!(
            "{method} requires committee authorization"
        )));
    }
    Ok(())
}
