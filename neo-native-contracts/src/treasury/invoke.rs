//! Treasury native-method dispatch.
//!
//! Keeps NEP payment callbacks and committee-witness verification routing out
//! of the contract root while preserving C# callback no-op behavior and
//! `Treasury.Verify` witness semantics.

use super::Treasury;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;

impl Treasury {
    pub(super) fn invoke_native(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            // Both callbacks are no-ops in C# (empty bodies); they return Void,
            // so an empty payload pushes nothing onto the stack.
            crate::NEP17_PAYMENT_METHOD | crate::NEP11_PAYMENT_METHOD => Ok(Vec::new()),
            // C# `Treasury.Verify` (Treasury.cs:41-42) = `CheckCommittee(engine)`:
            // true iff the committee multi-sig address witnesses the current
            // container - the witness boundary for Treasury-signed transactions.
            "verify" => {
                let authorized =
                    crate::committee::is_committee_witness(engine, "Treasury::verify")?;
                Ok(vec![u8::from(authorized)])
            }
            other => Err(CoreError::invalid_operation(format!(
                "Treasury method '{other}' is not implemented"
            ))),
        }
    }
}
