use super::CommandResult;
use crate::{
    commands::{contracts::ContractCommands, wallet::WalletCommands},
    console_service::ConsoleHelper,
};
use anyhow::{anyhow, bail};
use hex;
use neo_core::neo_vm::{stack_item::CompoundTypeExt, StackItem};
use neo_core::{
    big_decimal::BigDecimal,
    cryptography::crypto_utils::ECPoint,
    neo_system::NeoSystem,
    neo_vm::{op_code::OpCode, vm_state::VMState, ScriptBuilder},
    smart_contract::{
        application_engine::ApplicationEngine,
        call_flags::CallFlags,
        native::{NativeContract, NeoToken},
        trigger_type::TriggerType,
    },
    wallets::{asset_descriptor::AssetDescriptor, wallet_account::WalletAccount, Wallet},
    UInt160,
};
use serde_json::json;
use std::sync::Arc;

/// Governance commands (`MainService.Vote`).
pub struct VoteCommands {
    system: Arc<NeoSystem>,
    wallet: Arc<WalletCommands>,
    contracts: Arc<ContractCommands>,
}

impl VoteCommands {
    pub fn new(
        system: Arc<NeoSystem>,
        wallet: Arc<WalletCommands>,
        contracts: Arc<ContractCommands>,
    ) -> Self {
        Self {
            system,
            wallet,
            contracts,
        }
    }

    pub fn register_candidate(&self, account: &str) -> CommandResult {
        let script_hash = self.parse_hash_or_address(account)?;
        let wallet_account = self.load_wallet_account(&script_hash)?;
        let public_key = wallet_account
            .get_key()
            .ok_or_else(|| anyhow!("Account {} has no private key.", wallet_account.address()))?
            .public_key()
            .to_vec();
        let params = json!([{
            "type": "PublicKey",
            "value": hex::encode(public_key)
        }])
        .to_string();
        self.invoke_with_sender("registerCandidate", params, &script_hash, None)
    }

    pub fn unregister_candidate(&self, account: &str) -> CommandResult {
        let script_hash = self.parse_hash_or_address(account)?;
        let wallet_account = self.load_wallet_account(&script_hash)?;
        let public_key = wallet_account
            .get_key()
            .ok_or_else(|| anyhow!("Account {} has no private key.", wallet_account.address()))?
            .public_key()
            .to_vec();
        let params = json!([{
            "type": "PublicKey",
            "value": hex::encode(public_key)
        }])
        .to_string();
        self.invoke_with_sender("unregisterCandidate", params, &script_hash, None)
    }

    pub fn vote(&self, account: &str, public_key: &str) -> CommandResult {
        let script_hash = self.parse_hash_or_address(account)?;
        self.load_wallet_account(&script_hash)?;
        let normalized_key = self.normalize_public_key(public_key)?;
        let params = json!([
            {
                "type": "Hash160",
                "value": script_hash.to_string()
            },
            {
                "type": "PublicKey",
                "value": normalized_key
            }
        ])
        .to_string();
        self.invoke_with_sender("vote", params, &script_hash, None)
    }

    pub fn unvote(&self, account: &str) -> CommandResult {
        let script_hash = self.parse_hash_or_address(account)?;
        self.load_wallet_account(&script_hash)?;
        let params = json!([
            {
                "type": "Hash160",
                "value": script_hash.to_string()
            },
            {
                "type": "Any",
                "value": serde_json::Value::Null
            }
        ])
        .to_string();
        self.invoke_with_sender("vote", params, &script_hash, None)
    }

    pub fn get_candidates(&self) -> CommandResult {
        let result = self.invoke("getCandidates", Vec::new())?;
        let compound = result
            .as_compound()
            .map_err(|err| anyhow!(err.to_string()))?;
        if compound.count() == 0 {
            ConsoleHelper::warning("No candidates found.");
            return Ok(());
        }
        ConsoleHelper::info(["Candidates:"]);
        for entry in compound.sub_items() {
            let inner = entry
                .as_compound()
                .map_err(|err| anyhow!(err.to_string()))?;
            let items = inner.sub_items();
            if items.len() < 2 {
                continue;
            }
            let key_hex = hex::encode(
                items[0]
                    .as_bytes()
                    .map_err(|err| anyhow!(err.to_string()))?,
            );
            let votes = items[1].as_int().map_err(|err| anyhow!(err.to_string()))?;
            ConsoleHelper::info([&key_hex, "\t", &votes.to_string()]);
        }
        Ok(())
    }

    pub fn get_committee(&self) -> CommandResult {
        let result = self.invoke("getCommittee", Vec::new())?;
        let compound = result
            .as_compound()
            .map_err(|err| anyhow!(err.to_string()))?;
        if compound.count() == 0 {
            ConsoleHelper::warning("No committee members found.");
            return Ok(());
        }
        ConsoleHelper::info(["Committee:"]);
        for item in compound.sub_items() {
            let key_hex = hex::encode(item.as_bytes().map_err(|err| anyhow!(err.to_string()))?);
            ConsoleHelper::info([&key_hex]);
        }
        Ok(())
    }

    pub fn get_next_validators(&self) -> CommandResult {
        let result = self.invoke("getNextBlockValidators", Vec::new())?;
        let compound = result
            .as_compound()
            .map_err(|err| anyhow!(err.to_string()))?;
        if compound.count() == 0 {
            ConsoleHelper::warning("No validators available.");
            return Ok(());
        }
        ConsoleHelper::info(["Next validators:"]);
        for item in compound.sub_items() {
            let key_hex = hex::encode(item.as_bytes().map_err(|err| anyhow!(err.to_string()))?);
            ConsoleHelper::info([&key_hex]);
        }
        Ok(())
    }

    pub fn get_account_state(&self, account: &str) -> CommandResult {
        let script_hash = self.parse_hash_or_address(account)?;
        let arg = script_hash.to_bytes();
        let result = self.invoke("getAccountState", vec![arg])?;
        if let StackItem::Null = result {
            ConsoleHelper::warning("No vote record!");
            return Ok(());
        }
        let compound = result
            .as_compound()
            .map_err(|err| anyhow!(err.to_string()))?;
        let items = compound.sub_items();
        if items.len() < 3 {
            ConsoleHelper::warning("No vote record!");
            return Ok(());
        }

        let neo = NeoToken::new();
        let amount = BigDecimal::new(
            items[0].as_int().map_err(|err| anyhow!(err.to_string()))?,
            neo.decimals(),
        );
        let block = items[1].as_int().map_err(|err| anyhow!(err.to_string()))?;
        let pubkey = hex::encode(
            items[2]
                .as_bytes()
                .map_err(|err| anyhow!(err.to_string()))?,
        );
        ConsoleHelper::info(["Voted: ", &pubkey]);
        ConsoleHelper::info(["Amount: ", &amount.to_string()]);
        ConsoleHelper::info(["Block: ", &block.to_string()]);
        Ok(())
    }

    fn load_wallet_account(
        &self,
        script_hash: &UInt160,
    ) -> Result<Arc<dyn WalletAccount>, anyhow::Error> {
        let account = self
            .wallet
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?
            .get_account(script_hash)
            .ok_or_else(|| anyhow!("Account {} not found in the current wallet.", script_hash))?;
        if !account.has_key() {
            bail!(
                "Account {} does not contain a private key.",
                account.address()
            );
        }
        if account.is_locked() {
            bail!("Account {} is locked.", account.address());
        }
        Ok(account)
    }

    fn normalize_public_key(&self, input: &str) -> Result<String, anyhow::Error> {
        let cleaned = input.trim().trim_start_matches("0x");
        let bytes = hex::decode(cleaned)
            .map_err(|err| anyhow!("invalid public key '{}': {}", input, err))?;
        let point = ECPoint::from_bytes(&bytes)
            .map_err(|err| anyhow!("invalid public key '{}': {}", input, err))?;
        Ok(hex::encode(point.encoded()))
    }

    fn invoke_with_sender(
        &self,
        method: &str,
        params_json: String,
        sender: &UInt160,
        max_gas: Option<String>,
    ) -> CommandResult {
        let neo_hash = NeoToken::new().hash().to_string();
        let sender_str = sender.to_string();
        self.contracts.invoke(
            &neo_hash,
            method,
            Some(&params_json),
            Some(&sender_str),
            vec![sender_str.clone()],
            max_gas.as_deref(),
        )
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

    fn invoke(&self, method: &str, args: Vec<Vec<u8>>) -> Result<StackItem, anyhow::Error> {
        let store = self.system.store_cache();
        let neo_hash = NeoToken::new().hash();
        let mut builder = ScriptBuilder::new();
        for arg in args.iter().rev() {
            builder.emit_push(arg);
        }
        builder.emit_push_int(args.len() as i64);
        builder.emit_opcode(OpCode::PACK);
        builder.emit_push_int(CallFlags::READ_ONLY.bits() as i64);
        builder.emit_push(method.as_bytes());
        builder.emit_push(&neo_hash.to_bytes());
        builder
            .emit_syscall("System.Contract.Call")
            .map_err(|err| anyhow!(err))?;

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
            .load_script(builder.to_array(), CallFlags::READ_ONLY, Some(neo_hash))
            .map_err(|err| anyhow!(err))?;
        engine.execute().map_err(|err| anyhow!(err))?;
        if engine.state() != VMState::HALT {
            return Err(anyhow!("VM faulted while invoking {}", method));
        }
        let stack = engine.result_stack();
        stack
            .peek(0)
            .map(|item| item.clone())
            .map_err(|err| anyhow!(err.to_string()))
    }
}
