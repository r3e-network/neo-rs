use super::CommandResult;
use crate::console::{helper::StringPromptExt, percent::ConsolePercent};
use crate::console_service::ConsoleHelper;
use anyhow::{anyhow, bail};
use hex;
use neo_core::{
    big_decimal::BigDecimal,
    cryptography::crypto_utils::{ECCurve, ECPoint},
    ledger::{RelayResult, VerifyResult},
    neo_system::{NeoSystem, TransactionRouterMessage},
    neo_vm::{op_code::OpCode, ScriptBuilder},
    network::p2p::payloads::{
        signer::Signer, transaction::Transaction, transaction_attribute::TransactionAttribute,
    },
    network::payloads::conflicts::Conflicts,
    persistence::StoreCache,
    protocol_settings::ProtocolSettings,
    smart_contract::{
        contract::Contract,
        contract_parameters_context::ContractParametersContext,
        helper::Helper as ContractHelper,
        native::{
            contract_management::ContractManagement, GasToken, LedgerContract, NativeContract,
            NeoToken,
        },
    },
    wallets::{
        asset_descriptor::AssetDescriptor, helper::Helper as WalletHelper,
        transfer_output::TransferOutput, IWalletFactory, IWalletProvider, KeyPair, Nep6Wallet,
        Wallet, WalletAccount, WalletManager,
    },
    witness_scope::WitnessScope,
    UInt160, UInt256,
};
use neo_plugins::sqlite_wallet::SQLiteWalletFactory;
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use std::any::Any;
use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::{
    mpsc::{self, Receiver, Sender},
    Arc, Mutex, MutexGuard,
};
use tokio::runtime::Handle;
use uuid::Uuid;

/// Wallet management (`MainService.Wallet`).
#[allow(clippy::type_complexity)]
pub struct WalletCommands {
    settings: Arc<ProtocolSettings>,
    system: Arc<NeoSystem>,
    current_wallet: Mutex<Option<WalletHandle>>,
    wallet_event_sender: Mutex<Sender<Option<Arc<dyn Wallet>>>>,
    wallet_event_receiver: Mutex<Option<Receiver<Option<Arc<dyn Wallet>>>>>,
}

#[derive(Clone)]
struct WalletHandle {
    wallet: Nep6Wallet,
    path: PathBuf,
}

impl WalletCommands {
    pub fn new(settings: Arc<ProtocolSettings>, system: Arc<NeoSystem>) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            settings,
            system,
            current_wallet: Mutex::new(None),
            wallet_event_sender: Mutex::new(tx),
            wallet_event_receiver: Mutex::new(Some(rx)),
        }
    }

    /// Returns `true` when a wallet session is active.
    pub fn is_wallet_open(&self) -> bool {
        self.current_wallet
            .lock()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Returns the currently loaded wallet (cloned).
    pub fn current_wallet(&self) -> Option<Nep6Wallet> {
        self.current_wallet
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|handle| handle.wallet.clone()))
    }

    /// Returns the filesystem path of the opened wallet, if any.
    pub fn wallet_path(&self) -> Option<PathBuf> {
        self.current_wallet
            .lock()
            .ok()
            .and_then(|guard| guard.as_ref().map(|handle| handle.path.clone()))
    }

    /// Opens a wallet file (mirrors `MainService.OpenWallet`).
    pub fn open_wallet(&self, path: impl AsRef<Path>, password: &str) -> CommandResult {
        let path = path.as_ref();
        if !path.exists() {
            bail!("File does not exist: {}", path.display());
        }

        if password.is_empty() {
            bail!("wallet password cannot be empty");
        }

        if is_db3_wallet(path) {
            bail!("DB3 wallets are not supported yet; please migrate to a NEP-6 (.json) wallet.");
        }

        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("wallet path contains invalid UTF-8: {}", path.display()))?;

        let wallet = Nep6Wallet::from_file(path_str, password, self.settings.clone())
            .map_err(|err| anyhow!("failed to open wallet '{}': {err}", path.display()))?;

        let mut guard = self.lock_state()?;
        *guard = Some(WalletHandle {
            wallet,
            path: path.to_path_buf(),
        });
        if let Some(handle) = guard.as_ref() {
            self.emit_wallet_event(Some(&handle.wallet));
        }

        Ok(())
    }

    /// Closes the current wallet session (mirrors `MainService.OnCloseWalletCommand`).
    pub fn close_wallet(&self) -> CommandResult {
        let mut guard = self.lock_state()?;
        let closed = guard.take();
        if closed.is_some() {
            self.emit_wallet_event(None);
            Ok(())
        } else {
            bail!("You have to open the wallet first.");
        }
    }

    /// Stub upgrade command to mirror C# surface (DB3 migration not yet supported).
    pub fn upgrade_wallet(&self, path: &str) -> CommandResult {
        let extension = Path::new(path)
            .extension()
            .and_then(|ext| ext.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if extension != "db3" {
            bail!("Can't upgrade the wallet file. Check if your wallet is in db3 format.");
        }
        if !Path::new(path).exists() {
            bail!("File does not exist.");
        }
        let password = ConsoleHelper::read_user_input("password", true)?;
        if password.is_empty() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }

        let new_path = Path::new(path).with_extension("json");
        if new_path.exists() {
            bail!("File '{}' already exists", new_path.display());
        }

        let mut manager = WalletManager::default();
        manager.register_factory(Box::new(SQLiteWalletFactory));
        manager.register_factory(Box::new(Nep6WalletFactory));
        let migrated = Handle::current().block_on(manager.migrate_wallet(
            path,
            new_path.to_str().unwrap_or_default(),
            &password,
            &self.settings,
        ));

        match migrated {
            Ok(wallet) => {
                if let Err(err) = Handle::current().block_on(wallet.save()) {
                    bail!("failed to save migrated wallet: {}", err);
                }
                ConsoleHelper::info([
                    "Wallet file upgrade complete. New wallet file has been auto-saved at: ",
                    &new_path.display().to_string(),
                ]);
                Ok(())
            }
            Err(err) => bail!("DB3 wallet migration failed: {}", err.to_string()),
        }
    }

    /// Exports a DB3 wallet to a specified NEP-6 path without mutating the source.
    pub fn export_db3_wallet(&self, source: &str, destination: &str) -> CommandResult {
        let source_path = Path::new(source);
        if !source_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("db3"))
            .unwrap_or(false)
        {
            bail!("Source must be a DB3 wallet file.");
        }
        if !source_path.exists() {
            bail!("Source wallet does not exist.");
        }

        let dest_path = Path::new(destination);
        if dest_path.exists() {
            bail!(
                "Destination '{}' already exists; choose a new file path.",
                dest_path.display()
            );
        }

        let password = ConsoleHelper::read_user_input("password", true)?;
        if password.is_empty() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }

        let mut manager = WalletManager::default();
        manager.register_factory(Box::new(SQLiteWalletFactory));
        manager.register_factory(Box::new(Nep6WalletFactory));
        let migrated = Handle::current().block_on(manager.migrate_wallet(
            source,
            destination,
            &password,
            &self.settings,
        ));

        match migrated {
            Ok(wallet) => {
                if let Err(err) = Handle::current().block_on(wallet.save()) {
                    bail!("failed to save migrated wallet: {}", err);
                }
                ConsoleHelper::info([
                    "DB3 wallet exported successfully to: ",
                    &dest_path.display().to_string(),
                ]);
                Ok(())
            }
            Err(err) => bail!("DB3 wallet migration failed: {}", err.to_string()),
        }
    }

    /// Creates a new NEP-6 wallet and opens it (mirrors `CreateWallet`).
    pub fn create_wallet(&self, path: impl AsRef<Path>, password: &str) -> CommandResult {
        let path = path.as_ref();
        if path.exists() {
            bail!("wallet file already exists: {}", path.display());
        }

        if password.is_empty() {
            bail!("wallet password cannot be empty");
        }

        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| "wallet".to_string());
        let wallet = self.create_nep6_wallet(&name, path);
        let mut guard = self.lock_state()?;
        *guard = Some(WalletHandle {
            wallet,
            path: path.to_path_buf(),
        });
        if let Some(handle) = guard.as_ref() {
            self.emit_wallet_event(Some(&handle.wallet));
        }
        Ok(())
    }

    fn create_nep6_wallet(&self, name: &str, path: &Path) -> Nep6Wallet {
        Nep6Wallet::new(
            Some(name.to_string()),
            Some(path.to_string_lossy().to_string()),
            self.settings.clone(),
        )
    }

    fn lock_state(&self) -> Result<MutexGuard<'_, Option<WalletHandle>>, anyhow::Error> {
        self.current_wallet
            .lock()
            .map_err(|_| anyhow!("wallet state lock poisoned"))
    }

    fn emit_wallet_event(&self, wallet: Option<&Nep6Wallet>) {
        if let Ok(sender) = self.wallet_event_sender.lock() {
            let sender = sender.clone();
            let payload = wallet.map(|wallet| Arc::new(wallet.clone()) as Arc<dyn Wallet>);
            let _ = sender.send(payload);
        }
    }

    pub fn list_addresses(&self) -> CommandResult {
        let wallet = self
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        let store = self.system.store_cache();
        for account in wallet.get_accounts() {
            self.print_account(&store, &account)?;
        }
        Ok(())
    }

    fn print_account(&self, store: &StoreCache, account: &Arc<dyn WalletAccount>) -> CommandResult {
        let mut account_type = String::from("Nonstandard");
        let is_watch_only = !account.has_key();
        if is_watch_only {
            account_type = String::from("WatchOnly");
        } else if let Some(contract) = account.contract() {
            if ContractHelper::is_multi_sig_contract(&contract.script) {
                account_type = String::from("MultiSignature");
            } else if ContractHelper::is_signature_contract(&contract.script) {
                account_type = String::from("Standard");
            }
        }
        if account_type == "Nonstandard"
            && ContractManagement::get_contract_from_store_cache(store, &account.script_hash())
                .map_err(|err| anyhow!("failed to check deployed contract: {}", err))?
                .is_some()
        {
            account_type = String::from("Deployed-Nonstandard");
        }

        let address = account.address();
        ConsoleHelper::info([
            "   Address: ",
            address.as_str(),
            "\t",
            account_type.as_str(),
        ]);
        ConsoleHelper::info(["ScriptHash: ", &account.script_hash().to_string()]);
        ConsoleHelper::info([""]);
        Ok(())
    }

    pub fn list_assets(&self) -> CommandResult {
        let wallet = self
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        let store = self.system.store_cache();
        let neo = NeoToken::new();
        let gas = GasToken::new();
        let mut total_neo = BigInt::zero();
        let mut total_gas = BigInt::zero();

        for account in wallet.get_accounts() {
            let script_hash = account.script_hash();
            let address = account.address();
            let neo_balance = neo
                .balance_of_snapshot(&store, &script_hash)
                .map_err(|err| anyhow!("failed to read NEO balance: {}", err))?;
            let gas_balance = gas.balance_of_snapshot(&store, &script_hash);

            total_neo += neo_balance.clone();
            total_gas += gas_balance.clone();

            let neo_display = BigDecimal::new(neo_balance, neo.decimals());
            let gas_display = BigDecimal::new(gas_balance, gas.decimals());

            ConsoleHelper::info(["Address: ", address.as_str()]);
            ConsoleHelper::info(["NEO: ", &neo_display.to_string()]);
            ConsoleHelper::info(["GAS: ", &gas_display.to_string()]);
            ConsoleHelper::info([""]);
        }

        ConsoleHelper::info(["----------------------------------------------------"]);

        let total_neo_display = BigDecimal::new(total_neo.clone(), neo.decimals());
        let total_gas_display = BigDecimal::new(total_gas.clone(), gas.decimals());
        ConsoleHelper::info([
            "Total:   NEO: ",
            &total_neo_display.to_string(),
            "     GAS: ",
            &total_gas_display.to_string(),
        ]);
        ConsoleHelper::info([""]);
        ConsoleHelper::info(["NEO hash: ", &neo.hash().to_string()]);
        ConsoleHelper::info(["GAS hash: ", &gas.hash().to_string()]);
        Ok(())
    }

    pub fn list_keys(&self) -> CommandResult {
        let wallet = self
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        for account in wallet
            .get_accounts()
            .into_iter()
            .filter(|acct| acct.has_key())
        {
            let address = account.address();
            let script_hash = account.script_hash().to_string();
            ConsoleHelper::info(["   Address: ", address.as_str()]);
            ConsoleHelper::info(["ScriptHash: ", script_hash.as_str()]);
            if let Some(key) = account.get_key() {
                let public_key = hex::encode(key.public_key());
                ConsoleHelper::info([" PublicKey: ", public_key.as_str()]);
            }
            ConsoleHelper::info([""]);
        }
        Ok(())
    }

    pub fn create_addresses(&self, count: u16) -> CommandResult {
        if count == 0 {
            bail!("count must be greater than zero");
        }

        let output_path = Path::new("address.txt");
        if output_path.exists() {
            let prompt = format!(
                "The file '{}' already exists, do you want to overwrite it? (yes|no)",
                output_path.display()
            );
            let confirmation = ConsoleHelper::read_user_input(&prompt, false)?;
            if !confirmation.is_yes() {
                ConsoleHelper::info(["Cancelled"]);
                return Ok(());
            }
        }

        let mut guard = self.lock_state()?;
        let wallet_handle = guard
            .as_mut()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;

        let handle = Handle::current();
        let mut addresses = Vec::new();
        for _ in 0..count {
            let key = KeyPair::generate().map_err(|err| anyhow!("{}", err))?;
            let private_key = key.private_key();
            let account = handle
                .block_on(wallet_handle.wallet.create_account(&private_key))
                .map_err(|err| anyhow!("{}", err))?;
            addresses.push(account.address());
        }

        let _ = handle.block_on(wallet_handle.wallet.save());
        fs::write(output_path, addresses.join("\n"))?;
        ConsoleHelper::info(["Exported addresses to address.txt"]);
        Ok(())
    }

    pub fn delete_address(&self, address: &str) -> CommandResult {
        let mut guard = self.lock_state()?;
        let wallet_handle = guard
            .as_mut()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;

        let script_hash = WalletHelper::to_script_hash(address, self.settings.address_version)
            .map_err(|err| anyhow!(err))?;
        let handle = Handle::current();
        let removed = handle
            .block_on(wallet_handle.wallet.delete_account(&script_hash))
            .map_err(|err| anyhow!(err))?;
        if removed {
            let _ = handle.block_on(wallet_handle.wallet.save());
            ConsoleHelper::info(["Address deleted: ", address]);
        } else {
            ConsoleHelper::warning("Address not found in wallet.");
        }
        Ok(())
    }

    pub fn import_key(&self, input: &str) -> CommandResult {
        let mut guard = self.lock_state()?;
        let wallet_handle = guard
            .as_mut()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        let runtime = Handle::current();
        let path = Path::new(input);
        if path.is_file() {
            let contents = fs::read_to_string(path)
                .map_err(|err| anyhow!("failed to read {}: {}", path.display(), err))?;
            let lines: Vec<_> = contents.lines().map(str::trim).collect();
            let total = lines.len() as u64;
            let mut percent = ConsolePercent::new(0, total.max(1));
            for (idx, line) in lines.into_iter().enumerate() {
                self.import_key_entry(wallet_handle, &runtime, line)?;
                percent.set_value((idx + 1) as u64);
            }
        } else {
            self.import_key_entry(wallet_handle, &runtime, input.trim())?;
        }
        let _ = runtime.block_on(wallet_handle.wallet.save());
        Ok(())
    }

    fn import_key_entry(
        &self,
        wallet_handle: &mut WalletHandle,
        runtime: &Handle,
        entry: &str,
    ) -> CommandResult {
        if entry.is_empty() {
            return Ok(());
        }
        if Self::is_hex_key(entry) {
            let bytes = hex::decode(entry)
                .map_err(|err| anyhow!("invalid private key hex '{}': {}", entry, err))?;
            runtime
                .block_on(wallet_handle.wallet.create_account(&bytes))
                .map_err(|err| anyhow!("{}", err))?;
        } else {
            runtime
                .block_on(wallet_handle.wallet.import_wif(entry))
                .map_err(|err| anyhow!("{}", err))?;
        }
        Ok(())
    }

    pub fn import_watch_only(&self, input: &str) -> CommandResult {
        let mut guard = self.lock_state()?;
        let wallet_handle = guard
            .as_mut()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
        let runtime = Handle::current();
        let path = Path::new(input);
        if path.is_file() {
            let contents = fs::read_to_string(path)
                .map_err(|err| anyhow!("failed to read {}: {}", path.display(), err))?;
            let lines: Vec<_> = contents.lines().map(str::trim).collect();
            let total = lines.len() as u64;
            let mut percent = ConsolePercent::new(0, total.max(1));
            for (idx, line) in lines.into_iter().enumerate() {
                self.import_watch_entry(wallet_handle, &runtime, line)?;
                percent.set_value((idx + 1) as u64);
            }
        } else {
            self.import_watch_entry(wallet_handle, &runtime, input.trim())?;
        }
        let _ = runtime.block_on(wallet_handle.wallet.save());
        Ok(())
    }

    fn import_watch_entry(
        &self,
        wallet_handle: &mut WalletHandle,
        runtime: &Handle,
        entry: &str,
    ) -> CommandResult {
        if entry.is_empty() {
            return Ok(());
        }
        let script_hash = WalletHelper::to_script_hash(entry, self.settings.address_version)
            .map_err(|err| anyhow!("failed to parse watch-only address '{}': {}", entry, err))?;
        runtime
            .block_on(wallet_handle.wallet.create_account_watch_only(script_hash))
            .map_err(|err| anyhow!("{}", err))?;
        Ok(())
    }

    fn is_hex_key(text: &str) -> bool {
        text.len() == 64 && text.chars().all(|ch| ch.is_ascii_hexdigit())
    }

    pub fn export_keys(
        &self,
        script_hash: Option<&str>,
        path: Option<&str>,
        password: &str,
    ) -> CommandResult {
        let mut guard = self.lock_state()?;
        let wallet_handle = guard
            .as_mut()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;

        if let Some(path) = path {
            if Path::new(path).exists() {
                bail!("File '{}' already exists", path);
            }
        }

        if password.is_empty() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }

        let runtime = Handle::current();
        let verified = runtime
            .block_on(wallet_handle.wallet.verify_password(password))
            .map_err(|err| anyhow!("failed to verify password: {}", err))?;
        if !verified {
            bail!("Incorrect password");
        }

        let accounts = wallet_handle.wallet.get_accounts();
        let keys: Vec<String> = if let Some(value) = script_hash {
            let hash = self.parse_script_hash(value)?;
            accounts
                .into_iter()
                .filter(|account| account.script_hash() == hash)
                .filter_map(|account| account.get_key().map(|key| key.to_wif()))
                .collect()
        } else {
            accounts
                .into_iter()
                .filter_map(|account| account.get_key().map(|key| key.to_wif()))
                .collect()
        };

        if keys.is_empty() {
            ConsoleHelper::warning("No keys available for export.");
            return Ok(());
        }

        if let Some(path) = path {
            fs::write(path, keys.join("\n"))
                .map_err(|err| anyhow!("failed to write {}: {}", path, err))?;
        } else {
            for key in keys {
                ConsoleHelper::info([&key]);
            }
        }

        Ok(())
    }

    fn parse_script_hash(&self, input: &str) -> Result<UInt160, anyhow::Error> {
        if input.len() == 40 {
            input
                .parse::<UInt160>()
                .map_err(|err| anyhow!("invalid script hash '{}': {}", input, err))
        } else {
            WalletHelper::to_script_hash(input, self.settings.address_version)
                .map_err(|err| anyhow!("invalid address '{}': {}", input, err))
        }
    }

    pub fn import_multisig(&self, m: u16, keys: Vec<String>) -> CommandResult {
        if keys.is_empty() {
            bail!("import multisigaddress requires at least one public key");
        }
        if m == 0 {
            bail!("m must be greater than zero");
        }
        if keys.len() > 1024 {
            bail!("cannot import more than 1024 public keys");
        }

        let points = Self::parse_public_keys(&keys)?;
        if m as usize > points.len() {
            bail!("m cannot exceed the number of public keys");
        }

        let mut guard = self.lock_state()?;
        let wallet_handle = guard
            .as_mut()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;

        let contract = Contract::create_multi_sig_contract(m as usize, &points);
        let candidate = wallet_handle
            .wallet
            .get_accounts()
            .into_iter()
            .find_map(|account| {
                account.get_key().and_then(|key| {
                    key.get_public_key_point().ok().and_then(|point| {
                        if points.contains(&point) {
                            Some(key)
                        } else {
                            None
                        }
                    })
                })
            });

        let runtime = Handle::current();
        runtime
            .block_on(
                wallet_handle
                    .wallet
                    .create_account_with_contract(contract.clone(), candidate),
            )
            .map_err(|err| anyhow!("failed to import multisig: {}", err))?;
        let _ = runtime.block_on(wallet_handle.wallet.save());

        let address =
            WalletHelper::to_address(&contract.script_hash(), self.settings.address_version);
        ConsoleHelper::info(["Multisig address: ", &address]);
        Ok(())
    }

    fn parse_public_keys(keys: &[String]) -> Result<Vec<ECPoint>, anyhow::Error> {
        keys.iter()
            .map(|key| {
                let trimmed = key.trim().trim_start_matches("0x");
                let bytes = hex::decode(trimmed)
                    .map_err(|err| anyhow!("invalid public key '{}': {}", key, err))?;
                if bytes.len() == 33 {
                    ECPoint::decode_compressed(&bytes)
                        .map_err(|err| anyhow!("invalid public key '{}': {}", key, err))
                } else {
                    ECPoint::decode(&bytes, ECCurve::secp256r1())
                        .map_err(|err| anyhow!("invalid public key '{}': {}", key, err))
                }
            })
            .collect()
    }

    pub fn show_gas(&self) -> CommandResult {
        let wallet = self
            .current_wallet()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;

        let store = self.system.store_cache();
        let ledger = LedgerContract::new();
        let height = ledger
            .current_index(&store)
            .map_err(|err| anyhow!("failed to read current height: {}", err))?
            .saturating_add(1);
        let neo_token = NeoToken::new();
        let mut total = BigInt::zero();

        for account in wallet.get_accounts() {
            let script_hash = account.script_hash();
            let gas = neo_token
                .unclaimed_gas(&store, &script_hash, height)
                .map_err(|err| anyhow!("failed to compute unclaimed gas: {}", err))?;
            total += gas;
        }

        let decimals = GasToken::new().decimals();
        let formatted = BigDecimal::new(total, decimals);
        ConsoleHelper::info(["Unclaimed gas: ", &formatted.to_string()]);
        Ok(())
    }

    pub fn sign_context(&self, input: &str) -> CommandResult {
        let wallet = {
            let guard = self.lock_state()?;
            let handle = guard
                .as_ref()
                .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
            handle.wallet.clone()
        };

        let json_payload = self.read_context_input(input)?;
        let store_cache = self.system.store_cache();
        let snapshot_arc = Arc::new(store_cache.data_cache().clone());
        let (mut context, transaction) =
            ContractParametersContext::parse_transaction_context(&json_payload, snapshot_arc)
                .map_err(|err| anyhow!("failed to parse context: {}", err))?;

        if context.network != self.system.settings().network {
            ConsoleHelper::warning("Network mismatch.");
        }

        self.attach_wallet_signatures(&wallet, &mut context, &transaction)?;

        let json = context.to_json();
        let formatted = serde_json::to_string_pretty(&json).unwrap_or_else(|_| json.to_string());
        ConsoleHelper::info(["Signed Output: ", "\n", &formatted]);
        Ok(())
    }

    pub fn change_password(&self) -> CommandResult {
        let mut guard = self.lock_state()?;
        let wallet_handle = guard
            .as_mut()
            .ok_or_else(|| anyhow!("You have to open the wallet first."))?;

        let old_password = ConsoleHelper::read_user_input("password", true)?;
        if old_password.is_empty() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }

        let runtime = Handle::current();
        let verified = runtime
            .block_on(wallet_handle.wallet.verify_password(&old_password))
            .map_err(|err| anyhow!("failed to verify password: {}", err))?;
        if !verified {
            bail!("Incorrect password");
        }

        let new_password = ConsoleHelper::read_user_input("New password", true)?;
        let confirm_password = ConsoleHelper::read_user_input("Re-Enter Password", true)?;
        if new_password != confirm_password {
            bail!("Two passwords entered are inconsistent!");
        }
        if new_password.is_empty() {
            bail!("New password cannot be empty");
        }

        if let Some(path) = wallet_handle.wallet.path() {
            let backup = Path::new(path).with_extension("bak");
            if backup.exists() {
                bail!(
                    "Backup file '{}' already exists; remove it before changing password.",
                    backup.display()
                );
            }
            if let Err(err) = fs::copy(path, &backup) {
                bail!("Wallet backup failed: {}", err);
            }
        }

        let changed = runtime
            .block_on(
                wallet_handle
                    .wallet
                    .change_password(&old_password, &new_password),
            )
            .map_err(|err| anyhow!("failed to change password: {}", err))?;

        if changed {
            let _ = runtime.block_on(wallet_handle.wallet.save());
            ConsoleHelper::info(["Password changed successfully."]);
        } else {
            ConsoleHelper::warning("Password unchanged.");
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn send(
        &self,
        asset: &str,
        to: &str,
        amount: &str,
        from: Option<&str>,
        data: Option<&str>,
        signer_accounts: Vec<String>,
    ) -> CommandResult {
        let wallet = {
            let guard = self.lock_state()?;
            let handle = guard
                .as_ref()
                .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
            handle.wallet.clone()
        };

        let password = ConsoleHelper::read_user_input("password", true)?;
        if password.is_empty() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }

        let runtime = Handle::current();
        let verified = runtime
            .block_on(wallet.verify_password(&password))
            .map_err(|err| anyhow!("failed to verify password: {}", err))?;
        if !verified {
            bail!("Incorrect password");
        }

        let asset_hash = self.parse_asset_hash(asset)?;
        let recipient = self.parse_script_hash(to)?;
        let from_hash = from
            .and_then(|value| {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .map(|value| self.parse_script_hash(&value))
            .transpose()?;

        let signer_hashes = signer_accounts
            .into_iter()
            .map(|entry| entry.trim().to_string())
            .filter(|entry| !entry.is_empty())
            .map(|entry| self.parse_script_hash(&entry))
            .collect::<Result<Vec<_>, _>>()?;
        let cosigners: Vec<Signer> = signer_hashes
            .into_iter()
            .map(|account| Signer::new(account, WitnessScope::CALLED_BY_ENTRY))
            .collect();

        let transfer_data: Option<Box<dyn Any>> = data
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .map(|value| Box::new(value) as Box<dyn Any>);

        let store_cache = self.system.store_cache();
        let snapshot_arc = Arc::new(store_cache.data_cache().clone());
        let descriptor =
            AssetDescriptor::new(snapshot_arc.as_ref(), self.system.settings(), asset_hash)
                .map_err(|err| anyhow!("failed to read asset descriptor: {}", err))?;

        let (parsed, decimal_amount) = BigDecimal::try_parse(amount, descriptor.decimals);
        if !parsed || decimal_amount.sign() <= 0 {
            bail!("Incorrect amount format");
        }

        let transfer = TransferOutput {
            asset_id: asset_hash,
            value: decimal_amount,
            script_hash: recipient,
            data: transfer_data,
        };

        let signer_refs = if cosigners.is_empty() {
            None
        } else {
            Some(cosigners.as_slice())
        };

        let tx = WalletHelper::make_transfer_transaction(
            &wallet,
            snapshot_arc.as_ref(),
            &[transfer],
            from_hash,
            signer_refs,
            self.system.settings(),
            None,
            ContractHelper::MAX_VERIFICATION_GAS,
        )
        .map_err(|err| anyhow!("failed to create transaction: {}", err))?;

        let destination =
            WalletHelper::to_address(&recipient, self.system.settings().address_version);
        ConsoleHelper::info(["Send To: ", &destination]);

        let gas_decimals = GasToken::new().decimals();
        let network_fee = BigDecimal::new(BigInt::from(tx.network_fee()), gas_decimals).to_string();
        let total_fee = BigDecimal::new(
            BigInt::from(tx.system_fee() + tx.network_fee()),
            gas_decimals,
        )
        .to_string();
        ConsoleHelper::info([
            "Network fee: ",
            &format!("{network_fee} GAS\t"),
            "Total fee: ",
            &format!("{total_fee} GAS"),
        ]);

        let confirmation = ConsoleHelper::read_user_input("Relay tx? (no|yes)", false)?;
        if !confirmation.is_yes() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }

        let mut context = ContractParametersContext::new_with_type(
            snapshot_arc.clone(),
            tx.clone(),
            self.system.settings().network,
            Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
        );
        self.attach_wallet_signatures(&wallet, &mut context, &tx)?;
        if !context.completed() {
            let json = context.to_json().to_string();
            ConsoleHelper::info([
                "Transaction requires additional signatures. Context: ",
                &json,
            ]);
            bail!("transaction signing is incomplete");
        }

        self.sign_and_relay_with_context(tx, context)
    }

    pub fn cancel(
        &self,
        txid: &str,
        sender: Option<&str>,
        signer_accounts: Vec<String>,
    ) -> CommandResult {
        let wallet = {
            let guard = self.lock_state()?;
            let handle = guard
                .as_ref()
                .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
            handle.wallet.clone()
        };

        let hash =
            UInt256::from_str(txid).map_err(|err| anyhow!("invalid txid '{}': {}", txid, err))?;

        let store = self.system.store_cache();
        let ledger = LedgerContract::new();
        if ledger
            .get_transaction_state(&store, &hash)
            .map_err(|err| anyhow!("failed to query transaction state: {}", err))?
            .is_some()
        {
            bail!("This tx is already confirmed, can't be cancelled.");
        }

        let attributes = vec![TransactionAttribute::Conflicts(Conflicts { hash })];

        let sender_hash = sender
            .map(|value| self.parse_script_hash(value))
            .transpose()?;
        let mut signers: Vec<Signer> = Vec::new();
        if let Some(sender_hash) = sender_hash {
            let mut signer_hashes = signer_accounts
                .into_iter()
                .map(|entry| self.parse_script_hash(entry.trim()))
                .collect::<Result<Vec<_>, _>>()?;
            if signer_hashes.is_empty() {
                signer_hashes.push(sender_hash);
            } else if signer_hashes.contains(&sender_hash)
                && signer_hashes.first() != Some(&sender_hash)
            {
                signer_hashes.retain(|hash| hash != &sender_hash);
                signer_hashes.insert(0, sender_hash);
            } else if !signer_hashes.contains(&sender_hash) {
                signer_hashes.insert(0, sender_hash);
            }
            for account in signer_hashes {
                signers.push(Signer::new(account, WitnessScope::NONE));
            }
        }

        let mut script_builder = ScriptBuilder::new();
        script_builder.emit_opcode(OpCode::RET);
        let script = script_builder.to_array();

        let signer_refs = if signers.is_empty() {
            None
        } else {
            Some(signers.as_slice())
        };
        let mut tx = WalletHelper::make_transfer_transaction(
            &wallet,
            store.data_cache(),
            &[],
            sender_hash,
            signer_refs,
            self.system.settings(),
            None,
            ContractHelper::MAX_VERIFICATION_GAS,
        )
        .map_err(|err| anyhow!("failed to build cancel transaction: {}", err))?;
        tx.set_attributes(attributes.clone());
        tx.set_script(script);

        let conflict_tx = self
            .system
            .mempool()
            .lock()
            .ok()
            .and_then(|pool| pool.try_get(&hash));

        if let Some(conflict_tx) = conflict_tx {
            let adjusted_fee = tx
                .network_fee()
                .max(conflict_tx.network_fee())
                .saturating_add(1);
            tx.set_network_fee(adjusted_fee);
        } else {
            let descriptor = AssetDescriptor::new(
                store.data_cache(),
                self.system.settings(),
                GasToken::new().hash(),
            )
            .map_err(|err| anyhow!("failed to read GAS descriptor: {}", err))?;
            let extra_fee = ConsoleHelper::read_user_input(
                "This tx is not in mempool, please input extra fee (datoshi) manually",
                false,
            )?;
            let (ok, value) = BigDecimal::try_parse(&extra_fee, descriptor.decimals);
            if !ok || value.sign() <= 0 {
                bail!("Incorrect Amount Format");
            }
            let add_fee = value
                .value()
                .to_i64()
                .ok_or_else(|| anyhow!("failed to convert fee"))?;
            tx.set_network_fee(tx.network_fee().saturating_add(add_fee));
        }

        let gas_decimals = GasToken::new().decimals();
        let network_fee = BigDecimal::new(BigInt::from(tx.network_fee()), gas_decimals);
        let total_fee = BigDecimal::new(
            BigInt::from(tx.system_fee() + tx.network_fee()),
            gas_decimals,
        );
        ConsoleHelper::info([
            "Network fee: ",
            &format!("{} GAS\t", network_fee),
            "Total fee: ",
            &format!("{} GAS", total_fee),
        ]);

        let confirmation = ConsoleHelper::read_user_input("Relay tx? (no|yes)", false)?;
        if !confirmation.is_yes() {
            ConsoleHelper::info(["Cancelled"]);
            return Ok(());
        }

        let mut context = ContractParametersContext::new_with_type(
            Arc::new(store.data_cache().clone()),
            tx.clone(),
            self.system.settings().network,
            Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
        );
        self.attach_wallet_signatures(&wallet, &mut context, &tx)?;
        if !context.completed() {
            let json = context.to_json().to_string();
            ConsoleHelper::info([
                "Transaction requires additional signatures. Context: ",
                &json,
            ]);
            bail!("transaction signing is incomplete");
        }

        self.sign_and_relay_with_context(tx, context)
    }
    fn parse_asset_hash(&self, input: &str) -> Result<UInt160, anyhow::Error> {
        if input.eq_ignore_ascii_case("neo") {
            Ok(NeoToken::new().hash())
        } else if input.eq_ignore_ascii_case("gas") {
            Ok(GasToken::new().hash())
        } else {
            self.parse_script_hash(input)
        }
    }

    fn attach_wallet_signatures(
        &self,
        wallet: &Nep6Wallet,
        context: &mut ContractParametersContext,
        tx: &Transaction,
    ) -> CommandResult {
        for signer in tx.signers() {
            if let Some(account) = wallet.get_account(&signer.account) {
                let mut contract_opt = account.contract().cloned();
                let key_opt = account.get_key();
                if contract_opt.is_none() {
                    if let Some(ref key) = key_opt {
                        let point = key
                            .get_public_key_point()
                            .map_err(|err| anyhow!("failed to derive public key: {}", err))?;
                        contract_opt = Some(Contract::create_signature_contract(point));
                    }
                }

                if let Some(contract) = contract_opt {
                    context.add_contract(contract.clone());
                    if let Some(key) = key_opt {
                        if account.has_key() && !account.is_locked() {
                            let signature =
                                WalletHelper::sign(tx, &key, self.system.settings().network)
                                    .map_err(|err| anyhow!(err))?;
                            let pub_key = ECPoint::new(key.compressed_public_key());
                            let _ = context.add_signature(contract.clone(), pub_key, signature);
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn sign_and_relay_with_context(
        &self,
        mut tx: Transaction,
        context: ContractParametersContext,
    ) -> CommandResult {
        if let Some(witnesses) = context.get_witnesses() {
            tx.set_witnesses(witnesses);
        } else {
            bail!("failed to build witnesses");
        }
        self.relay_transaction(tx)
    }

    pub fn sign_and_relay(&self, tx: Transaction) -> CommandResult {
        let wallet = {
            let guard = self.lock_state()?;
            let handle = guard
                .as_ref()
                .ok_or_else(|| anyhow!("You have to open the wallet first."))?;
            handle.wallet.clone()
        };

        let store = self.system.store_cache();
        let mut context = ContractParametersContext::new_with_type(
            Arc::new(store.data_cache().clone()),
            tx.clone(),
            self.system.settings().network,
            Some("Neo.Network.P2P.Payloads.Transaction".to_string()),
        );
        self.attach_wallet_signatures(&wallet, &mut context, &tx)?;
        if !context.completed() {
            let json = context.to_json().to_string();
            ConsoleHelper::info([
                "Transaction requires additional signatures. Context: ",
                &json,
            ]);
            bail!("transaction signing is incomplete");
        }

        self.sign_and_relay_with_context(tx, context)
    }

    fn relay_transaction(&self, tx: Transaction) -> CommandResult {
        let cloned = tx.clone();
        let result = self.with_relay_responder(|sender| {
            self.system
                .tx_router_actor()
                .tell_from(
                    TransactionRouterMessage::Preverify {
                        transaction: cloned,
                        relay: true,
                    },
                    Some(sender),
                )
                .map_err(|err| anyhow!("failed to submit transaction: {}", err))
        })?;
        self.map_relay_result(result)
    }

    fn with_relay_responder<F>(&self, send: F) -> Result<RelayResult, anyhow::Error>
    where
        F: FnOnce(akka::ActorRef) -> Result<(), anyhow::Error>,
    {
        use akka::{Actor, ActorContext, ActorResult, Props};
        use async_trait::async_trait;
        use std::sync::{mpsc::channel, Arc as StdArc, Mutex as StdMutex};

        struct RelayResponder {
            tx: StdArc<StdMutex<Option<std::sync::mpsc::Sender<RelayResult>>>>,
        }

        #[async_trait]
        impl Actor for RelayResponder {
            async fn pre_start(&mut self, _ctx: &mut ActorContext) -> ActorResult {
                Ok(())
            }

            async fn handle(
                &mut self,
                msg: Box<dyn Any + Send>,
                _ctx: &mut ActorContext,
            ) -> ActorResult {
                if let Ok(result) = msg.downcast::<RelayResult>() {
                    if let Some(sender) = self.tx.lock().unwrap().take() {
                        let _ = sender.send(*result);
                    }
                }
                Ok(())
            }
        }

        let (tx, rx) = channel();
        let responder = RelayResponder {
            tx: StdArc::new(StdMutex::new(Some(tx))),
        };
        let actor_ref = self
            .system
            .actor_system()
            .actor_of(
                Props::new(move || RelayResponder {
                    tx: StdArc::clone(&responder.tx),
                }),
                format!("wallet_relay_responder_{}", Uuid::new_v4()),
            )
            .map_err(|err| anyhow!("failed to create relay responder: {}", err))?;

        send(actor_ref.clone())?;

        let result = rx
            .recv()
            .map_err(|err| anyhow!("failed to receive relay result: {}", err))?;
        Ok(result)
    }

    fn map_relay_result(&self, result: RelayResult) -> CommandResult {
        match result.result {
            VerifyResult::Succeed => {
                ConsoleHelper::info([
                    "Transaction relayed successfully. Hash: ",
                    &result.hash.to_string(),
                ]);
                Ok(())
            }
            VerifyResult::AlreadyExists => {
                bail!("Transaction already exists on the blockchain.")
            }
            VerifyResult::AlreadyInPool => bail!("Transaction already exists in the mempool."),
            VerifyResult::OutOfMemory => bail!("Mempool capacity reached."),
            VerifyResult::InvalidScript => bail!("Transaction script is invalid."),
            VerifyResult::InvalidAttribute => bail!("Transaction contains invalid attributes."),
            VerifyResult::InvalidSignature => bail!("Transaction contains invalid signatures."),
            VerifyResult::OverSize => bail!("Transaction exceeds the allowed size."),
            VerifyResult::Expired => bail!("Transaction has already expired."),
            VerifyResult::InsufficientFunds => {
                bail!("Insufficient funds for the requested transfer.")
            }
            VerifyResult::PolicyFail => bail!("Transaction rejected by policy."),
            VerifyResult::UnableToVerify => bail!("Transaction cannot be verified at this time."),
            VerifyResult::Invalid => bail!("Transaction verification failed."),
            VerifyResult::HasConflicts => {
                bail!("Transaction conflicts with an existing mempool entry.")
            }
            VerifyResult::Unknown => {
                bail!("Transaction verification failed for an unknown reason.")
            }
        }
    }

    fn read_context_input(&self, input: &str) -> Result<String, anyhow::Error> {
        let path = Path::new(input);
        if path.exists() && path.is_file() {
            fs::read_to_string(path)
                .map_err(|err| anyhow!("failed to read {}: {}", path.display(), err))
        } else {
            Ok(input.to_string())
        }
    }
}

impl IWalletProvider for WalletCommands {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn wallet_changed(&self) -> Receiver<Option<Arc<dyn Wallet>>> {
        self.wallet_event_receiver
            .lock()
            .expect("wallet event receiver poisoned")
            .take()
            .expect("wallet event receiver already taken")
    }

    fn get_wallet(&self) -> Option<Arc<dyn Wallet>> {
        self.current_wallet()
            .map(|wallet| Arc::new(wallet) as Arc<dyn Wallet>)
    }
}

fn is_db3_wallet(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("db3"))
        .unwrap_or(false)
}

#[derive(Default)]
struct Nep6WalletFactory;

impl IWalletFactory for Nep6WalletFactory {
    fn handle(&self, path: &str) -> bool {
        Path::new(path)
            .extension()
            .map(|ext| ext.eq_ignore_ascii_case("json"))
            .unwrap_or(false)
    }

    fn create_wallet(
        &self,
        name: &str,
        path: &str,
        _password: &str,
        settings: &ProtocolSettings,
    ) -> Result<Box<dyn Wallet>, String> {
        let wallet = Nep6Wallet::new(
            Some(name.to_string()),
            Some(path.to_string()),
            Arc::new(settings.clone()),
        );
        Ok(Box::new(wallet))
    }

    fn open_wallet(
        &self,
        path: &str,
        password: &str,
        settings: &ProtocolSettings,
    ) -> Result<Box<dyn Wallet>, String> {
        let wallet = Nep6Wallet::from_file(path, password, Arc::new(settings.clone()))
            .map_err(|err| err.to_string())?;
        Ok(Box::new(wallet))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wallet_commands() -> WalletCommands {
        let settings = Arc::new(ProtocolSettings::default());
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
        WalletCommands::new(settings, system)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn close_wallet_requires_session() {
        let commands = wallet_commands();
        let err = commands.close_wallet().unwrap_err();
        assert!(err
            .to_string()
            .contains("You have to open the wallet first"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn open_wallet_requires_existing_file() {
        let commands = wallet_commands();
        let err = commands
            .open_wallet("missing.json", "password")
            .unwrap_err();
        assert!(err.to_string().contains("File does not exist"));
    }
}
