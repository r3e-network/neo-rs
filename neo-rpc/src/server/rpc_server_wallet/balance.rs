use std::sync::Arc;

use neo_execution::application_engine::ApplicationEngine;
use neo_manifest::CallFlags;
use neo_primitives::{BigDecimal, TriggerType, UInt160};
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::{OpCode, VmState as VMState};
use neo_wallets::Wallet as CoreWallet;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};

use super::RpcServerWallet;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, invalid_params};
use crate::server::rpc_server::RpcServer;

impl RpcServerWallet {
    pub(super) fn calculate_nep17_balance<W>(
        server: &RpcServer,
        wallet: &W,
        asset: &UInt160,
    ) -> Result<BigDecimal, RpcException>
    where
        W: CoreWallet + ?Sized,
    {
        let accounts: Vec<UInt160> = wallet
            .accounts()
            .into_iter()
            .filter(|account| account.has_key())
            .map(|account| account.script_hash())
            .collect();
        if accounts.is_empty() {
            return Ok(Self::zero_balance());
        }

        let script = Self::build_balance_script(asset, &accounts)?;
        let store = server.system().store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            snapshot,
            None,
            server.system().settings().as_ref().clone(),
            server.settings().max_gas_invoke,
            None,
        )
        .map_err(|err| internal_error(err.to_string()))?;
        engine
            .load_script(script, CallFlags::READ_ONLY, Some(*asset))
            .map_err(|err| internal_error(err.to_string()))?;
        // C# `Wallet.GetBalance` runs the probe with
        // `ApplicationEngine.Run` and reports a zero balance when the
        // engine faults; on HALT it reads the result stack (decimals
        // on top, then the summed amount).
        if engine.execute_allow_fault() == VMState::FAULT {
            return Ok(Self::zero_balance());
        }
        let decimals_value = engine
            .result_stack()
            .peek(0)
            .map_err(|err| internal_error(err.to_string()))?
            .as_int()
            .map_err(|err| internal_error(err.to_string()))?;
        let decimals = decimals_value
            .to_u8()
            .ok_or_else(|| invalid_params("invalid decimals value"))?;
        let amount_value = engine
            .result_stack()
            .peek(1)
            .map_err(|err| internal_error(err.to_string()))?
            .as_int()
            .map_err(|err| internal_error(err.to_string()))?;
        Ok(BigDecimal::new(amount_value, decimals))
    }

    fn build_balance_script(
        asset: &UInt160,
        accounts: &[UInt160],
    ) -> Result<Vec<u8>, RpcException> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        for account in accounts {
            let account_bytes = account.to_bytes();
            Self::emit_contract_call(
                &mut builder,
                asset,
                "balanceOf",
                &[account_bytes.as_slice()],
                CallFlags::READ_ONLY,
            )?;
            builder.emit_opcode(OpCode::ADD);
        }
        Self::emit_contract_call(&mut builder, asset, "decimals", &[], CallFlags::READ_ONLY)?;
        Ok(builder.to_array())
    }

    fn emit_contract_call(
        builder: &mut ScriptBuilder,
        contract: &UInt160,
        method: &str,
        args: &[&[u8]],
        flags: CallFlags,
    ) -> Result<(), RpcException> {
        if args.is_empty() {
            builder.emit_opcode(OpCode::NEWARRAY0);
        } else {
            for arg in args.iter().rev() {
                builder.emit_push(arg);
            }
            builder.emit_push_int(args.len() as i64);
            builder.emit_opcode(OpCode::PACK);
        }

        builder.emit_push_int(i64::from(flags.bits()));
        builder.emit_push(method.as_bytes());
        let hash_bytes = contract.to_bytes();
        builder.emit_push(&hash_bytes);
        builder
            .emit_syscall("System.Contract.Call")
            .map_err(|err| internal_error(err.to_string()))?;
        Ok(())
    }

    fn zero_balance() -> BigDecimal {
        BigDecimal::new(BigInt::zero(), 0)
    }
}
