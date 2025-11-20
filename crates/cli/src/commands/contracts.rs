use super::CommandResult;
use crate::{
    commands::wallet::WalletCommands,
    console::helper::{ContractScriptValidator, StringPromptExt},
    console_service::ConsoleHelper,
};
use anyhow::{anyhow, bail, Context};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use neo_core::neo_vm::{
    call_flags::CallFlags, op_code::OpCode, vm_state::VMState, ScriptBuilder, StackItem,
};
use neo_core::{
    big_decimal::BigDecimal,
    cryptography::crypto_utils::{ECCurve, ECPoint},
    neo_system::NeoSystem,
    network::p2p::payloads::{
        signer::Signer,
        transaction::{Transaction, MAX_TRANSACTION_SIZE},
    },
    persistence::DataCache,
    protocol_settings::ProtocolSettings,
    smart_contract::{
        contract_parameter::{ContractParameter, ContractParameterValue},
        contract_parameter_type::ContractParameterType,
        contract_state::{ContractState, NefFile},
        helper::Helper as ContractHelper,
        json_serializer::JsonSerializer,
        manifest::{ContractAbi, ContractManifest, ContractParameterDefinition},
        native::{contract_management::ContractManagement, GasToken},
        trigger_type::TriggerType,
    },
    wallets::{helper::Helper as WalletHelper, Nep6Wallet, Wallet},
    witness_scope::WitnessScope,
    UInt160, UInt256,
};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use rand::Rng;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

/// Smart-contract deployment/invocation commands (`MainService.Contracts`).
pub struct ContractCommands {
    system: Arc<NeoSystem>,
    wallet: Arc<WalletCommands>,
    settings: Arc<ProtocolSettings>,
}

impl ContractCommands {
    pub fn new(system: Arc<NeoSystem>, wallet: Arc<WalletCommands>) -> Self {
        Self {
            settings: Arc::new(system.settings().clone()),
            system,
            wallet,
        }
    }

    /// Deploys a contract (`deploy <nef> [manifest] [data]`).
    pub fn deploy(
        &self,
        nef_path: &str,
        manifest_path: Option<&str>,
        data: Option<&str>,
    ) -> CommandResult {
        let wallet = self
            .wallet
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        let manifest_path = self.resolve_manifest_path(nef_path, manifest_path);

        let (nef, manifest) = self.load_nef_and_manifest(nef_path, &manifest_path)?;
        self.validate_script(&nef, &manifest)?;

        let script = self
            .build_deploy_script(&nef, &manifest, data)
            .context("failed to build script")?;

        let tx = self
            .build_transaction(
                &wallet,
                script,
                None,
                Vec::new(),
                ContractHelper::MAX_VERIFICATION_GAS,
            )
            .context("failed to build deployment transaction")?;

        let sender = tx
            .signers()
            .first()
            .map(|signer| signer.account)
            .ok_or_else(|| anyhow!("transaction has no signer"))?;
        let contract_hash =
            ContractHelper::get_contract_hash(&sender, nef.checksum, &manifest.name);
        self.print_deploy_info(&tx, contract_hash);

        let confirmation = ConsoleHelper::read_user_input("Relay tx? (no|yes)", false)?;
        if !confirmation.is_yes() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }

        self.wallet.sign_and_relay(tx)
    }

    /// Invokes a contract method (mirrors `MainService.Contracts.OnInvokeCommand`).
    #[allow(clippy::too_many_arguments)]
    pub fn invoke(
        &self,
        script_hash: &str,
        operation: &str,
        parameters_json: Option<&str>,
        sender: Option<&str>,
        signer_accounts: Vec<String>,
        max_gas: Option<&str>,
    ) -> CommandResult {
        let wallet = self
            .wallet
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        let contract_hash = self.parse_hash(script_hash)?;
        let args = self.parse_parameter_list(parameters_json)?;
        let gas_limit = self.parse_gas_amount(max_gas)?;
        self.ensure_contract_method(&contract_hash, operation, args.len())?;
        let sender_hash = sender.map(|value| self.parse_hash(value)).transpose()?;
        let (sender_override, signers) = self.prepare_signers(sender_hash, signer_accounts)?;
        let script = self.build_invoke_script(&contract_hash, operation, &args)?;
        self.print_script_bytes(&script);
        self.preview_script(&script, gas_limit)?;

        let tx = self
            .build_transaction(&wallet, script, sender_override, signers, gas_limit)
            .context("failed to build invocation transaction")?;
        self.print_fee_summary(&tx);

        let confirmation = ConsoleHelper::read_user_input("Relay tx? (no|yes)", false)?;
        if !confirmation.is_yes() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }
        self.wallet.sign_and_relay(tx)
    }

    /// Test invokes a contract method without creating a transaction.
    pub fn test_invoke(
        &self,
        script_hash: &str,
        operation: &str,
        parameters_json: Option<&str>,
        max_gas: Option<&str>,
    ) -> CommandResult {
        let contract_hash = self.parse_hash(script_hash)?;
        let args = self.parse_parameter_list(parameters_json)?;
        let gas_limit = self.parse_gas_amount(max_gas)?;
        self.ensure_contract_method(&contract_hash, operation, args.len())?;
        let script = self.build_invoke_script(&contract_hash, operation, &args)?;
        self.print_script_bytes(&script);
        self.preview_script(&script, gas_limit)
    }

    /// Invokes a contract method using ABI parsing.
    #[allow(clippy::too_many_arguments)]
    pub fn invoke_abi(
        &self,
        script_hash: &str,
        operation: &str,
        abi_args: Option<&str>,
        sender: Option<&str>,
        signer_accounts: Vec<String>,
        max_gas: Option<&str>,
    ) -> CommandResult {
        let contract_hash = self.parse_hash(script_hash)?;
        let contract = ContractManagement::get_contract_from_store_cache(
            &self.system.store_cache(),
            &contract_hash,
        )
        .map_err(|err| anyhow!("failed to load contract state: {}", err))?
        .ok_or_else(|| anyhow!("Contract does not exist."))?;
        if contract.manifest.abi.methods.is_empty() {
            bail!("Contract ABI is not available.");
        }

        let args_value = match abi_args
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            Some(text) => serde_json::from_str::<Value>(text)
                .map_err(|err| anyhow!("failed to parse args JSON: {}", err))?,
            None => Value::Array(Vec::new()),
        };

        let (parameters_json, _decoded_params) =
            self.parse_parameters_from_abi(&contract.manifest.abi, operation, &args_value)?;

        self.invoke(
            script_hash,
            operation,
            parameters_json.as_deref(),
            sender,
            signer_accounts,
            max_gas,
        )
    }

    /// Updates an existing contract (`update <scriptHash> <nef> <manifest> <sender> [signers] [data]`).
    pub fn update(
        &self,
        script_hash: &str,
        nef_path: &str,
        manifest_path: &str,
        sender: &str,
        signer_accounts: Vec<String>,
        data: Option<&str>,
    ) -> CommandResult {
        let wallet = self
            .wallet
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        let contract_hash = self.parse_hash(script_hash)?;
        let sender_hash = self.parse_hash(sender)?;
        let (sender_override, signers) =
            self.prepare_signers(Some(sender_hash), signer_accounts)?;

        let (nef, manifest) = self.load_nef_and_manifest(nef_path, Path::new(manifest_path))?;
        self.validate_script(&nef, &manifest)?;
        let script = self
            .build_update_script(&contract_hash, &nef, &manifest, data)
            .context("failed to build update script")?;

        let tx = self
            .build_transaction(
                &wallet,
                script,
                sender_override,
                signers,
                ContractHelper::MAX_VERIFICATION_GAS,
            )
            .context("failed to build update transaction")?;

        let store = self.system.store_cache();
        if let Some(state) =
            ContractManagement::get_contract_from_store_cache(&store, &contract_hash)
                .map_err(|err| anyhow!("failed to load contract state: {}", err))?
        {
            self.print_update_info(&tx, &state);
        } else {
            ConsoleHelper::warning(format!(
                "Can't upgrade, contract hash not found: {}",
                contract_hash
            ));
            return Ok(());
        }

        let confirmation = ConsoleHelper::read_user_input("Relay tx? (no|yes)", false)?;
        if !confirmation.is_yes() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }

        self.wallet.sign_and_relay(tx)
    }

    fn resolve_manifest_path(&self, nef_path: &str, manifest_path: Option<&str>) -> PathBuf {
        manifest_path.map(PathBuf::from).unwrap_or_else(|| {
            let mut default = PathBuf::from(nef_path);
            default.set_extension("manifest.json");
            default
        })
    }

    fn load_nef_and_manifest(
        &self,
        nef_path: &str,
        manifest_path: &Path,
    ) -> Result<(NefFile, ContractManifest), anyhow::Error> {
        let nef_bytes = self.read_file(nef_path)?;
        if nef_bytes.len() >= MAX_TRANSACTION_SIZE {
            bail!(
                "Contract NEF file size ({} bytes) exceeds maximum transaction size ({MAX_TRANSACTION_SIZE} bytes)",
                nef_bytes.len()
            );
        }
        let nef = NefFile::parse(&nef_bytes)
            .map_err(|err| anyhow!("failed to parse NEF file '{}': {}", nef_path, err))?;

        if !manifest_path.exists() {
            bail!(
                "Contract manifest file not found at path: {}",
                manifest_path.display()
            );
        }

        let manifest_bytes = self.read_file(manifest_path)?;
        if manifest_bytes.len() >= MAX_TRANSACTION_SIZE {
            bail!(
                "Contract manifest file size ({} bytes) exceeds maximum transaction size ({MAX_TRANSACTION_SIZE} bytes)",
                manifest_bytes.len()
            );
        }

        let manifest_json =
            std::str::from_utf8(&manifest_bytes).context("manifest file is not valid UTF-8")?;
        let manifest = ContractManifest::from_json_str(manifest_json)
            .map_err(|err| anyhow!("failed to parse manifest: {}", err))?;
        Ok((nef, manifest))
    }

    fn read_file<P: AsRef<Path>>(&self, path: P) -> Result<Vec<u8>, anyhow::Error> {
        fs::read(&path).with_context(|| format!("failed to read {}", path.as_ref().display()))
    }

    fn validate_script(&self, nef: &NefFile, manifest: &ContractManifest) -> CommandResult {
        ContractScriptValidator::validate(&nef.script, &manifest.abi)
            .map_err(|err| anyhow!("script validation failed: {}", err))
    }

    fn build_deploy_script(
        &self,
        nef: &NefFile,
        manifest: &ContractManifest,
        data: Option<&str>,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let args = self.compose_contract_args(nef, manifest, data)?;
        let mut builder = ScriptBuilder::new();
        Self::emit_contract_call(
            &mut builder,
            &ContractManagement::contract_hash(),
            "deploy",
            &args,
        )?;
        Ok(builder.to_array())
    }

    fn build_update_script(
        &self,
        script_hash: &UInt160,
        nef: &NefFile,
        manifest: &ContractManifest,
        data: Option<&str>,
    ) -> Result<Vec<u8>, anyhow::Error> {
        let args = self.compose_contract_args(nef, manifest, data)?;
        let mut builder = ScriptBuilder::new();
        Self::emit_contract_call(&mut builder, script_hash, "update", &args)?;
        Ok(builder.to_array())
    }

    fn build_invoke_script(
        &self,
        script_hash: &UInt160,
        method: &str,
        args: &[StackItem],
    ) -> Result<Vec<u8>, anyhow::Error> {
        let mut builder = ScriptBuilder::new();
        Self::emit_contract_call(&mut builder, script_hash, method, args)?;
        Ok(builder.to_array())
    }

    fn compose_contract_args(
        &self,
        nef: &NefFile,
        manifest: &ContractManifest,
        data: Option<&str>,
    ) -> Result<Vec<StackItem>, anyhow::Error> {
        let manifest_value = manifest
            .to_json()
            .map_err(|err| anyhow!("failed to serialize manifest: {}", err))?;
        let manifest_json =
            serde_json::to_string(&manifest_value).expect("manifest serialization must succeed");

        let mut args = vec![
            StackItem::from_byte_string(nef.to_bytes()),
            StackItem::from_byte_string(manifest_json.into_bytes()),
        ];

        if let Some(value) = data.and_then(|text| {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        }) {
            let json: Value = serde_json::from_str(value)
                .map_err(|err| anyhow!("failed to parse data JSON: {}", err))?;
            let stack_item =
                JsonSerializer::deserialize_from_json(&json, 16).map_err(|err| anyhow!(err))?;
            args.push(stack_item);
        }

        Ok(args)
    }

    fn emit_contract_call(
        builder: &mut ScriptBuilder,
        script_hash: &UInt160,
        method: &str,
        args: &[StackItem],
    ) -> Result<(), anyhow::Error> {
        if args.is_empty() {
            builder.emit_opcode(OpCode::NEWARRAY0);
        } else {
            let count = args.len();
            for item in args.iter().rev() {
                builder
                    .emit_push_stack_item(item.clone())
                    .map_err(|err| anyhow!("failed to emit argument: {}", err))?;
            }
            builder.emit_push_int(count as i64);
            builder.emit_pack();
        }

        builder.emit_push_int(CallFlags::ALL.bits() as i64);
        builder.emit_push(method.as_bytes());
        builder.emit_push(&script_hash.to_bytes());
        builder
            .emit_syscall("System.Contract.Call")
            .map_err(|err| anyhow!("failed to emit syscall: {}", err))?;
        Ok(())
    }

    fn build_transaction(
        &self,
        wallet: &Nep6Wallet,
        script: Vec<u8>,
        sender_override: Option<UInt160>,
        signers: Vec<Signer>,
        max_gas: i64,
    ) -> Result<Transaction, anyhow::Error> {
        let mut tx = Transaction::new();
        tx.set_script(script);
        tx.set_nonce(rand::thread_rng().gen::<u32>());

        let final_signers = if signers.is_empty() {
            let sender = sender_override
                .or_else(|| self.select_signer(wallet))
                .ok_or_else(|| anyhow!("wallet contains no unlocked accounts"))?;
            vec![Signer::new(sender, WitnessScope::CALLED_BY_ENTRY)]
        } else {
            signers
        };
        tx.set_signers(final_signers);

        let store_cache = self.system.store_cache();
        let ledger = neo_core::smart_contract::native::LedgerContract::new();
        let policy = neo_core::smart_contract::native::PolicyContract::new();
        let current_height = ledger
            .current_index(&store_cache)
            .map_err(|err| anyhow!("failed to query current height: {}", err))?;
        let increment = policy
            .get_max_valid_until_block_increment_snapshot(&store_cache, &self.settings)
            .unwrap_or(self.settings.max_valid_until_block_increment);
        tx.set_valid_until_block(current_height + increment);

        self.evaluate_fees(wallet, store_cache.data_cache(), &mut tx, max_gas)?;
        Ok(tx)
    }

    fn select_signer(&self, wallet: &Nep6Wallet) -> Option<UInt160> {
        wallet
            .get_accounts()
            .into_iter()
            .find(|account| account.has_key() && !account.is_locked())
            .map(|account| account.script_hash())
    }

    fn parse_signer_list(
        &self,
        sender: UInt160,
        entries: Vec<String>,
    ) -> Result<Vec<UInt160>, anyhow::Error> {
        let mut hashes = Vec::new();
        for entry in entries {
            let trimmed = entry.trim();
            if trimmed.is_empty() {
                continue;
            }
            hashes.push(self.parse_hash(trimmed)?);
        }

        if hashes.is_empty() {
            hashes.push(sender);
        } else if let Some(pos) = hashes.iter().position(|hash| *hash == sender) {
            if pos != 0 {
                hashes.remove(pos);
                hashes.insert(0, sender);
            }
        } else {
            hashes.insert(0, sender);
        }
        Ok(hashes)
    }

    fn prepare_signers(
        &self,
        sender: Option<UInt160>,
        entries: Vec<String>,
    ) -> Result<(Option<UInt160>, Vec<Signer>), anyhow::Error> {
        if let Some(sender_hash) = sender {
            let hashes = self.parse_signer_list(sender_hash, entries)?;
            let signers = hashes
                .into_iter()
                .map(|hash| Signer::new(hash, WitnessScope::CALLED_BY_ENTRY))
                .collect();
            Ok((Some(sender_hash), signers))
        } else if entries.is_empty() {
            Ok((None, Vec::new()))
        } else {
            let mut hashes = Vec::new();
            for entry in entries {
                let trimmed = entry.trim();
                if trimmed.is_empty() {
                    continue;
                }
                hashes.push(self.parse_hash(trimmed)?);
            }
            let signers = hashes
                .into_iter()
                .map(|hash| Signer::new(hash, WitnessScope::CALLED_BY_ENTRY))
                .collect();
            Ok((None, signers))
        }
    }

    fn parse_hash(&self, input: &str) -> Result<UInt160, anyhow::Error> {
        let trimmed = input.trim();
        if trimmed.is_empty() {
            bail!("script hash or address cannot be empty");
        }

        if trimmed.starts_with("0x") || trimmed.len() == 40 {
            UInt160::from_str(trimmed)
                .map_err(|err| anyhow!("invalid script hash '{}': {}", trimmed, err))
        } else {
            WalletHelper::to_script_hash(trimmed, self.settings.address_version)
                .map_err(|err| anyhow!("invalid address '{}': {}", trimmed, err))
        }
    }

    fn parse_parameter_list(&self, json: Option<&str>) -> Result<Vec<StackItem>, anyhow::Error> {
        let trimmed = json.map(|text| text.trim()).filter(|text| !text.is_empty());
        let value = match trimmed {
            Some(text) => serde_json::from_str::<Value>(text)
                .map_err(|err| anyhow!("failed to parse parameters JSON: {}", err))?,
            None => Value::Array(Vec::new()),
        };

        let array = value
            .as_array()
            .ok_or_else(|| anyhow!("parameters JSON must be an array"))?;
        let mut items = Vec::new();
        for entry in array {
            let parameter = ContractParameter::from_json(entry)
                .map_err(|err| anyhow!("invalid parameter: {}", err))?;
            items.push(self.parameter_to_stack_item(&parameter)?);
        }
        Ok(items)
    }

    fn ensure_contract_method(
        &self,
        script_hash: &UInt160,
        operation: &str,
        parameter_count: usize,
    ) -> CommandResult {
        let store = self.system.store_cache();
        let contract = ContractManagement::get_contract_from_store_cache(&store, script_hash)
            .map_err(|err| anyhow!("failed to load contract state: {}", err))?
            .ok_or_else(|| anyhow!("Contract does not exist."))?;
        let exists =
            contract.manifest.abi.methods.iter().any(|method| {
                method.name == operation && method.parameters.len() == parameter_count
            });
        if !exists {
            bail!("This method does not exist in this contract.");
        }
        Ok(())
    }

    fn parameter_to_stack_item(
        &self,
        parameter: &ContractParameter,
    ) -> Result<StackItem, anyhow::Error> {
        use ContractParameterValue::*;
        let item = match &parameter.value {
            Any | Void => StackItem::Null,
            Boolean(value) => StackItem::from_bool(*value),
            Integer(value) => StackItem::from_int(value.clone()),
            Hash160(value) => StackItem::from_byte_string(value.to_array()),
            Hash256(value) => StackItem::from_byte_string(value.to_array()),
            ByteArray(bytes) | Signature(bytes) => StackItem::from_byte_string(bytes.clone()),
            PublicKey(point) => StackItem::from_byte_string(point.encoded()),
            String(text) => StackItem::from_byte_string(text.as_bytes()),
            Array(values) => {
                let mut converted = Vec::with_capacity(values.len());
                for value in values {
                    converted.push(self.parameter_to_stack_item(value)?);
                }
                StackItem::from_array(converted)
            }
            Map(entries) => {
                let mut map = BTreeMap::new();
                for (key, value) in entries {
                    let key_item = self.parameter_to_stack_item(key)?;
                    let value_item = self.parameter_to_stack_item(value)?;
                    map.insert(key_item, value_item);
                }
                StackItem::from_map(map)
            }
            InteropInterface => {
                bail!("InteropInterface parameters are not supported in CLI invoke")
            }
        };
        Ok(item)
    }

    fn evaluate_fees(
        &self,
        wallet: &Nep6Wallet,
        snapshot: &DataCache,
        tx: &mut Transaction,
        max_gas: i64,
    ) -> Result<(), anyhow::Error> {
        let mut engine = neo_core::smart_contract::application_engine::ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(snapshot.clone()),
            None,
            self.settings.as_ref().clone(),
            max_gas,
            None,
        )
        .map_err(|err| anyhow!("failed to create application engine: {}", err))?;
        engine
            .load_script(tx.script().to_vec(), CallFlags::ALL, None)
            .map_err(|err| anyhow!("failed to load deployment script: {}", err))?;
        engine
            .execute()
            .map_err(|err| anyhow!("failed to execute deployment script: {}", err))?;
        if engine.state() == VMState::FAULT {
            return Err(anyhow!("Smart contract execution failed."));
        }
        tx.set_system_fee(engine.fee_consumed());

        let network_fee = WalletHelper::calculate_network_fee_with_wallet(
            tx,
            snapshot,
            self.settings.as_ref(),
            Some(wallet),
            ContractHelper::MAX_VERIFICATION_GAS,
        )
        .map_err(|err| anyhow!(err))?;
        tx.set_network_fee(network_fee);
        Ok(())
    }

    fn print_deploy_info(&self, tx: &Transaction, contract_hash: UInt160) {
        ConsoleHelper::info(["Contract hash: ", &contract_hash.to_string()]);
        self.print_fee_summary(tx);
    }

    fn print_update_info(&self, tx: &Transaction, contract: &ContractState) {
        ConsoleHelper::info(["Contract hash: ", &contract.hash.to_string()]);
        ConsoleHelper::info(["Updated times: ", &contract.update_counter.to_string()]);
        self.print_fee_summary(tx);
    }

    fn print_fee_summary(&self, tx: &Transaction) {
        let decimals = GasToken::new().decimals();
        let system_fee = BigDecimal::new(BigInt::from(tx.system_fee()), decimals);
        let network_fee = BigDecimal::new(BigInt::from(tx.network_fee()), decimals);
        let total_fee = BigDecimal::new(BigInt::from(tx.system_fee() + tx.network_fee()), decimals);
        ConsoleHelper::info(["Gas consumed: ", &format!("{} GAS", system_fee)]);
        ConsoleHelper::info(["Network fee: ", &format!("{} GAS", network_fee)]);
        ConsoleHelper::info(["Total fee: ", &format!("{} GAS", total_fee)]);
    }

    fn print_script_bytes(&self, script: &[u8]) {
        let encoded = BASE64_STANDARD.encode(script);
        ConsoleHelper::info(["Invoking script with: ", &format!("'{encoded}'")]);
    }

    fn preview_script(&self, script: &[u8], gas_limit: i64) -> CommandResult {
        let store_cache = self.system.store_cache();
        let mut engine = neo_core::smart_contract::application_engine::ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::new(store_cache.data_cache().clone()),
            None,
            self.settings.as_ref().clone(),
            gas_limit,
            None,
        )
        .map_err(|err| anyhow!("failed to create execution engine: {}", err))?;
        engine
            .load_script(script.to_vec(), CallFlags::ALL, None)
            .map_err(|err| anyhow!("failed to load invocation script: {}", err))?;
        engine
            .execute()
            .map_err(|err| anyhow!("failed to execute invocation script: {}", err))?;
        self.print_execution_output(&engine, true);
        if engine.state() == VMState::FAULT {
            if let Some(message) = engine.fault_exception() {
                bail!(message.to_string());
            } else {
                bail!("VM faulted during invocation");
            }
        }
        Ok(())
    }

    fn print_execution_output(
        &self,
        engine: &neo_core::smart_contract::application_engine::ApplicationEngine,
        show_stack: bool,
    ) {
        ConsoleHelper::info(["VM State: ", &format!("{:?}", engine.state())]);
        let gas = BigDecimal::new(
            BigInt::from(engine.fee_consumed()),
            GasToken::new().decimals(),
        );
        ConsoleHelper::info(["Gas Consumed: ", &format!("{}", gas)]);

        if show_stack {
            let stack = engine.result_stack();
            let mut items = Vec::new();
            for index in 0..stack.len() {
                if let Ok(item) = stack.peek(index) {
                    if let Ok(json) = JsonSerializer::serialize_to_json(item) {
                        items.push(json);
                    }
                }
            }
            if let Ok(text) = serde_json::to_string(&items) {
                ConsoleHelper::info(["Result Stack: ", &text]);
            }
        }
    }

    fn parse_gas_amount(&self, gas: Option<&str>) -> Result<i64, anyhow::Error> {
        let decimals = GasToken::new().decimals();
        let trimmed = gas.map(|value| value.trim()).unwrap_or("20");
        let big_decimal = if trimmed.is_empty() {
            BigDecimal::new(BigInt::from(20), 0)
                .change_decimals(decimals)
                .map_err(|err| anyhow!(err.to_string()))?
        } else {
            BigDecimal::parse(trimmed, decimals)
                .map_err(|err| anyhow!("invalid gas amount: {}", err))?
        };
        big_decimal
            .value()
            .to_i64()
            .ok_or_else(|| anyhow!("gas amount is out of range"))
    }
    fn parse_parameters_from_abi(
        &self,
        abi: &ContractAbi,
        operation: &str,
        args: &Value,
    ) -> Result<(Option<String>, Vec<ContractParameter>), anyhow::Error> {
        let array = args
            .as_array()
            .ok_or_else(|| anyhow!("ABI arguments JSON must be an array"))?;
        let mut parameters = Vec::new();
        let method = abi
            .methods
            .iter()
            .find(|method| method.name == operation)
            .ok_or_else(|| anyhow!("Method '{}' does not exist in this contract.", operation))?;
        if method.parameters.len() != array.len() {
            bail!(
                "Method '{}' expects {} parameters but {} were provided.",
                operation,
                method.parameters.len(),
                array.len()
            );
        }

        for (index, descriptor) in method.parameters.iter().enumerate() {
            let value = array.get(index).ok_or_else(|| {
                anyhow!(
                    "Missing parameter at index {} while parsing ABI arguments",
                    index
                )
            })?;
            let parameter = self.parse_parameter_from_abi(descriptor, value)?;
            parameters.push(parameter);
        }

        let serialized = serde_json::to_string(
            &parameters
                .iter()
                .map(|parameter| parameter.to_json())
                .collect::<Vec<_>>(),
        )
        .ok();
        Ok((serialized, parameters))
    }

    fn parse_parameter_from_abi(
        &self,
        descriptor: &ContractParameterDefinition,
        value: &Value,
    ) -> Result<ContractParameter, anyhow::Error> {
        use ContractParameterType as Type;

        let parameter = match descriptor.param_type {
            Type::Boolean => ContractParameter::with_value(
                Type::Boolean,
                ContractParameterValue::Boolean(value.as_bool().ok_or_else(|| {
                    anyhow!(
                        "Parameter '{}' expects a boolean but received {}.",
                        descriptor.name,
                        value
                    )
                })?),
            ),
            Type::Integer => ContractParameter::with_value(
                Type::Integer,
                ContractParameterValue::Integer(
                    BigInt::from_str(value.as_str().ok_or_else(|| {
                        anyhow!(
                            "Parameter '{}' expects a numeric string but received {}.",
                            descriptor.name,
                            value
                        )
                    })?)
                    .map_err(|_| {
                        anyhow!(
                            "Parameter '{}' expects a numeric string but received {}.",
                            descriptor.name,
                            value
                        )
                    })?,
                ),
            ),
            Type::String => ContractParameter::with_value(
                Type::String,
                ContractParameterValue::String(
                    value
                        .as_str()
                        .ok_or_else(|| {
                            anyhow!(
                                "Parameter '{}' expects a string but received {}.",
                                descriptor.name,
                                value
                            )
                        })?
                        .to_string(),
                ),
            ),
            Type::Hash160 => ContractParameter::with_value(
                Type::Hash160,
                ContractParameterValue::Hash160(self.parse_hash(value.as_str().ok_or_else(
                    || {
                        anyhow!(
                            "Parameter '{}' expects a script hash string but received {}.",
                            descriptor.name,
                            value
                        )
                    },
                )?)?),
            ),
            Type::Hash256 => ContractParameter::with_value(
                Type::Hash256,
                ContractParameterValue::Hash256(
                    UInt256::from_str(value.as_str().ok_or_else(|| {
                        anyhow!(
                            "Parameter '{}' expects a hash string but received {}.",
                            descriptor.name,
                            value
                        )
                    })?)
                    .map_err(|err| {
                        anyhow!("Invalid hash value for '{}': {}", descriptor.name, err)
                    })?,
                ),
            ),
            Type::ByteArray | Type::Signature => ContractParameter::with_value(
                descriptor.param_type,
                ContractParameterValue::ByteArray(
                    BASE64_STANDARD
                        .decode(value.as_str().ok_or_else(|| {
                            anyhow!(
                                "Parameter '{}' expects base64 string but received {}.",
                                descriptor.name,
                                value
                            )
                        })?)
                        .map_err(|err| {
                            anyhow!(
                                "Parameter '{}' has invalid byte sequence: {}",
                                descriptor.name,
                                err
                            )
                        })?,
                ),
            ),
            Type::PublicKey => ContractParameter::with_value(
                Type::PublicKey,
                ContractParameterValue::PublicKey(
                    Self::decode_public_key(value.as_str().ok_or_else(|| {
                        anyhow!(
                            "Parameter '{}' expects a public key string but received {}.",
                            descriptor.name,
                            value
                        )
                    })?)
                    .ok_or_else(|| anyhow!("invalid public key for '{}'", descriptor.name))?,
                ),
            ),
            Type::Array => {
                let array = value
                    .as_array()
                    .ok_or_else(|| anyhow!("Parameter '{}' expects an array.", descriptor.name))?;
                let mut items = Vec::new();
                for item in array {
                    let converted =
                        ContractParameter::from_json(item).map_err(|err| anyhow!(err))?;
                    items.push(converted);
                }
                ContractParameter::with_value(Type::Array, ContractParameterValue::Array(items))
            }
            Type::Map => {
                let array = value
                    .as_array()
                    .ok_or_else(|| anyhow!("Parameter '{}' expects an array.", descriptor.name))?;
                let mut entries = Vec::new();
                for item in array {
                    let key = ContractParameter::from_json(
                        item.get("key")
                            .ok_or_else(|| anyhow!("Map entry missing 'key'"))?,
                    )
                    .map_err(|err| anyhow!(err))?;
                    let value = ContractParameter::from_json(
                        item.get("value")
                            .ok_or_else(|| anyhow!("Map entry missing 'value'"))?,
                    )
                    .map_err(|err| anyhow!(err))?;
                    entries.push((key, value));
                }
                ContractParameter::with_value(Type::Map, ContractParameterValue::Map(entries))
            }
            Type::Any => ContractParameter::from_json(value).map_err(|err| {
                anyhow!("failed to parse parameter '{}': {}", descriptor.name, err)
            })?,
            Type::InteropInterface => {
                bail!(
                    "InteropInterface parameter '{}' is not supported",
                    descriptor.name
                )
            }
            Type::Void => bail!("Void parameter '{}' is not supported", descriptor.name),
        };
        Ok(parameter)
    }

    fn decode_public_key(input: &str) -> Option<ECPoint> {
        let cleaned = input.trim().trim_start_matches("0x");
        let bytes = hex::decode(cleaned).ok()?;
        if bytes.len() == 33 {
            ECPoint::decode_compressed(&bytes).ok()
        } else {
            ECPoint::decode(&bytes, ECCurve::secp256r1()).ok()
        }
    }
}
