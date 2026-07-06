//! Treasury native-method handlers.
//!
//! Keeps NEP payment callbacks and committee-witness verification bodies out of
//! the contract root while preserving C# callback no-op behavior and
//! `Treasury.Verify` witness semantics. Dispatch is declared by the metadata
//! binding table and `native_contract_dispatch!`.

use super::Treasury;
use neo_error::CoreResult;
use neo_execution::ApplicationEngine;

impl Treasury {
    pub(super) fn invoke_nep_payment(
        &self,
        _engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // Both callbacks are no-ops in C# (empty bodies); they return Void,
        // so an empty payload pushes nothing onto the stack.
        Ok(Vec::new())
    }

    pub(super) fn invoke_verify(
        &self,
        engine: &mut ApplicationEngine,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        // C# `Treasury.Verify` (Treasury.cs:41-42) = `CheckCommittee(engine)`:
        // true iff the committee multi-sig address witnesses the current
        // container - the witness boundary for Treasury-signed transactions.
        let authorized = crate::committee::is_committee_witness(engine, "Treasury::verify")?;
        Ok(vec![u8::from(authorized)])
    }
}
