use std::sync::Arc;

use neo_execution::application_engine::ApplicationEngine;
use neo_primitives::CallFlags;
use neo_primitives::{BigDecimal, TriggerType, UInt160};
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::{OpCode, VmState as VMState};
use neo_wallets::{Wallet as CoreWallet, WalletAccount};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use serde_json::Value;

use super::RpcServerWallet;
use super::request::{NoParamsRequest, WalletBalanceRequest};
use super::response::{wallet_balance_to_json, wallet_unclaimed_gas_to_json};
use crate::server::ledger_queries;
use crate::server::native_queries;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{internal_error, invalid_params};
use crate::server::rpc_server::RpcServer;

impl RpcServerWallet {
    pub(super) fn get_wallet_balance(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let request = WalletBalanceRequest::parse(params)?;
        let wallet = Self::require_wallet(server)?;
        // C# GetWalletBalance sums per-account `balanceOf` script probes
        // (Wallet.GetAvailable). The engine-script path below invokes the
        // same native `balanceOf` / `decimals` methods for every NEP-17
        // asset, NEO and GAS included.
        let balance = Self::calculate_nep17_balance(server, wallet.as_ref(), &request.asset)?;
        Ok(wallet_balance_to_json(&balance))
    }

    pub(super) fn get_wallet_unclaimed_gas(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getwalletunclaimedgas")?;
        let wallet = Self::require_wallet(server)?;
        let store = server.system().store_cache();
        let height = ledger_queries::current_index(store.data_cache())
            .map_err(internal_error)?
            .saturating_add(1);
        let neo_hash = native_queries::NativeQueries::neo_script_hash();
        let snapshot = Arc::new(store.data_cache().clone());
        let mut total = BigInt::zero();
        for account in wallet.accounts() {
            // C# GetWalletUnclaimedGas sums NativeContract.NEO.UnclaimedGas
            // per account; the engine probe invokes the same native
            // `unclaimedGas(account, end)` method.
            let gas = crate::server::native_queries::NativeQueries::neo_unclaimed_gas(
                server,
                Arc::clone(&snapshot),
                &neo_hash,
                &account.script_hash(),
                height,
            )
            .map_err(internal_error)?;
            total += gas;
        }
        Ok(wallet_unclaimed_gas_to_json(&total))
    }

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
        let system = server.system();
        let store = system.store_cache();
        let snapshot = Arc::new(store.data_cache().clone());
        let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
            TriggerType::Application,
            None,
            snapshot,
            None,
            system.settings().as_ref().clone(),
            server.settings().max_gas_invoke,
            neo_execution::NoDiagnostic,
            system.native_contract_provider(),
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
