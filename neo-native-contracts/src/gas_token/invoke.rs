//! GAS native-method dispatch.
//!
//! Keeps NEP-17 ABI method routing out of the contract root while preserving
//! the shared transfer core, storage accounting, notifications, and callback
//! queueing behavior.

use super::GasToken;
use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_primitives::UInt160;
use num_bigint::BigInt;

impl GasToken {
    pub(super) fn invoke_native(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(BigInt::from(Self::DECIMALS).to_signed_bytes_le()),
            "totalSupply" => {
                let snapshot = engine.snapshot_cache();
                Ok(Self::total_supply(&snapshot).to_signed_bytes_le())
            }
            "balanceOf" => {
                let account = crate::args::raw_account(args, "GasToken::balanceOf")?;
                let snapshot = engine.snapshot_cache();
                Ok(Self::balance_of(&snapshot, &account)?.to_signed_bytes_le())
            }
            "transfer" => {
                // C# FungibleToken.Transfer(from, to, amount, data).
                let from = crate::args::raw_hash160(args, 0, "GasToken::transfer")?;
                let to = crate::args::raw_hash160(args, 1, "GasToken::transfer")?;
                let amount = crate::args::raw_required_integer_arg(
                    args,
                    2,
                    "GasToken::transfer",
                    "an amount",
                )?;
                let data = args.get(3).map(Vec::as_slice).unwrap_or(&[]);
                // The witness bypass uses the engine's calling script hash
                // (C# `from.Equals(CallingScriptHash)`).
                let caller = engine
                    .get_calling_script_hash()
                    .unwrap_or_else(UInt160::zero);
                Ok(vec![u8::from(Self::transfer_core(
                    engine, caller, &from, &to, &amount, data,
                )?)])
            }
            other => Err(CoreError::invalid_operation(format!(
                "GasToken method '{other}' is not implemented"
            ))),
        }
    }
}
