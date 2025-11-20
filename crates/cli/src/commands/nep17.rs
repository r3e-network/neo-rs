use super::CommandResult;
use crate::console_service::ConsoleHelper;
use anyhow::anyhow;
use neo_core::{
    big_decimal::BigDecimal,
    neo_system::NeoSystem,
    neo_vm::{op_code::OpCode, vm_state::VMState, ScriptBuilder},
    smart_contract::{
        application_engine::ApplicationEngine,
        call_flags::CallFlags,
        native::{ContractManagement, GasToken, NativeContract, NeoToken},
        trigger_type::TriggerType,
    },
    wallets::asset_descriptor::AssetDescriptor,
    UInt160,
};
use num_bigint::BigInt;
use std::sync::Arc;

use super::wallet::WalletCommands;

/// NEP-17 token helpers (`MainService.NEP17`).
pub struct Nep17Commands {
    system: Arc<NeoSystem>,
    wallet: Arc<WalletCommands>,
}

impl Nep17Commands {
    pub fn new(system: Arc<NeoSystem>, wallet: Arc<WalletCommands>) -> Self {
        Self { system, wallet }
    }

    pub fn balance_of(&self, token: &str, account: &str) -> CommandResult {
        let token_hash = self.parse_hash_or_alias(token)?;
        let account_hash = self.parse_hash_or_address(account)?;

        let store = self.system.store_cache();
        let asset = AssetDescriptor::new(store.data_cache(), self.system.settings(), token_hash)
            .map_err(|err| anyhow!("failed to read asset descriptor: {}", err))?;

        let balance =
            self.invoke_integer(&token_hash, "balanceOf", vec![account_hash.to_bytes()])?;
        let value = BigDecimal::new(balance, asset.decimals);
        ConsoleHelper::info(["", &format!("{} balance: {}", asset.asset_name, value)]);
        Ok(())
    }

    pub fn transfer(
        &self,
        token: &str,
        to: &str,
        amount: &str,
        from: Option<&str>,
        data: Option<&str>,
        signer_accounts: Vec<String>,
    ) -> CommandResult {
        // Delegate to wallet send to reuse signing/relay flow.
        self.wallet
            .send(token, to, amount, from, data, signer_accounts)
    }

    pub fn name(&self, token: &str) -> CommandResult {
        let token_hash = self.parse_hash_or_alias(token)?;
        let store = self.system.store_cache();
        let contract = ContractManagement::get_contract_from_store_cache(&store, &token_hash)
            .map_err(|err| anyhow!("failed to read contract: {}", err))?;
        if let Some(contract) = contract {
            ConsoleHelper::info(["Result: ", &contract.manifest.name]);
            Ok(())
        } else {
            Err(anyhow!("Contract hash not found: {}", token_hash))
        }
    }

    pub fn decimals(&self, token: &str) -> CommandResult {
        let token_hash = self.parse_hash_or_alias(token)?;
        let decimals = self.invoke_integer(&token_hash, "decimals", Vec::new())?;
        ConsoleHelper::info(["Result: ", &decimals.to_string()]);
        Ok(())
    }

    pub fn total_supply(&self, token: &str) -> CommandResult {
        let token_hash = self.parse_hash_or_alias(token)?;
        let store = self.system.store_cache();
        let asset = AssetDescriptor::new(store.data_cache(), self.system.settings(), token_hash)
            .map_err(|err| anyhow!("failed to read asset descriptor: {}", err))?;
        let supply = self.invoke_integer(&token_hash, "totalSupply", Vec::new())?;
        let formatted = BigDecimal::new(supply, asset.decimals);
        ConsoleHelper::info(["Result: ", &formatted.to_string()]);
        Ok(())
    }

    fn parse_hash_or_alias(&self, input: &str) -> Result<UInt160, anyhow::Error> {
        if input.eq_ignore_ascii_case("neo") {
            Ok(NeoToken::new().hash())
        } else if input.eq_ignore_ascii_case("gas") {
            Ok(GasToken::new().hash())
        } else {
            input.parse::<UInt160>().map_err(|err| anyhow!(err))
        }
    }

    fn parse_hash_or_address(&self, input: &str) -> Result<UInt160, anyhow::Error> {
        if input.len() == 40 {
            input.parse::<UInt160>().map_err(|err| anyhow!(err))
        } else {
            let version = self.system.settings().address_version;
            neo_core::wallets::helper::Helper::to_script_hash(input, version)
                .map_err(|err| anyhow!(err))
        }
    }

    fn invoke_integer(
        &self,
        script_hash: &UInt160,
        method: &str,
        args: Vec<Vec<u8>>,
    ) -> Result<BigInt, anyhow::Error> {
        let store = self.system.store_cache();
        let mut builder = ScriptBuilder::new();
        Self::emit_call(&mut builder, script_hash, method.as_bytes(), &args)?;
        let script = builder.to_array();

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(store.data_cache().clone()),
            None,
            self.system.settings().clone(),
            AssetDescriptor::QUERY_GAS,
            None,
        )
        .map_err(|err| anyhow!(err))?;
        engine
            .load_script(script, CallFlags::READ_ONLY, Some(*script_hash))
            .map_err(|err| anyhow!(err))?;
        engine.execute().map_err(|err| anyhow!(err))?;
        if engine.state() != VMState::HALT {
            return Err(anyhow!("VM faulted while invoking {}", method));
        }
        let stack = engine.result_stack();
        if stack.len() < 1 {
            return Err(anyhow!("Contract call returned empty stack"));
        }
        let item = stack.peek(0).map_err(|err| anyhow!(err.to_string()))?;
        item.as_int().map_err(|err| anyhow!(err.to_string()))
    }

    fn emit_call(
        builder: &mut ScriptBuilder,
        script_hash: &UInt160,
        method: &[u8],
        args: &[Vec<u8>],
    ) -> Result<(), anyhow::Error> {
        for arg in args.iter().rev() {
            builder.emit_push(arg);
        }
        builder.emit_push_int(args.len() as i64);
        builder.emit_opcode(OpCode::PACK);
        builder.emit_push_int(CallFlags::READ_ONLY.bits() as i64);
        builder.emit_push(method);
        builder.emit_push(&script_hash.to_bytes());
        builder
            .emit_syscall("System.Contract.Call")
            .map_err(|err| anyhow!(err))?;
        Ok(())
    }
}
