use super::contract_management::ContractManagement;
use super::fungible_token::{FungibleToken, PREFIX_ACCOUNT as ACCOUNT_PREFIX, PREFIX_TOTAL_SUPPLY};
use super::native_contract::{NativeContract, NativeMethod};
use super::policy_contract::PolicyContract;
use super::security_fixes::{
    PermissionValidator, ReentrancyGuardType, SafeArithmetic, SecurityContext, StateValidator,
};
use super::AccountState;
use crate::error::{CoreError, CoreResult};
use crate::network::p2p::payloads::{Transaction, TransactionAttribute, TransactionAttributeType};
use crate::persistence::read_only_store::ReadOnlyStoreGeneric;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::storage_context::StorageContext;
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::StorageItem;
use crate::vm_runtime::StackItem;
use crate::UInt160;
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use once_cell::sync::Lazy;
use std::sync::OnceLock;

static GAS_HASH: Lazy<UInt160> =
    Lazy::new(|| Helper::get_contract_hash(&UInt160::zero(), 0, "GasToken"));

/// GAS native token with NEP-17 compliant behaviour.
pub struct GasToken {
    methods: Vec<NativeMethod>,
}

impl Default for GasToken {
    fn default() -> Self {
        Self::new()
    }
}

impl GasToken {
    const ID: i32 = -6;
    const SYMBOL: &'static str = "GAS";
    const DECIMALS: u8 = 8;
    const NAME: &'static str = "GasToken";

    pub fn new() -> Self {
        Self {
            methods: Self::native_methods(),
        }
    }

    pub fn symbol(&self) -> &'static str {
        Self::SYMBOL
    }

    pub fn decimals(&self) -> u8 {
        Self::DECIMALS
    }

    fn invoke_method(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(vec![Self::DECIMALS]),
            "totalSupply" => {
                let snapshot = engine.snapshot_cache();
                let total = self.total_supply_snapshot(snapshot.as_ref());
                Ok(Self::encode_amount(&total))
            }
            "balanceOf" => self.balance_of(engine, args),
            "transfer" => self.transfer(engine, args),
            _ => Err(CoreError::native_contract(format!(
                "Unknown method: {}",
                method
            ))),
        }
    }

    fn balance_of(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        if args.len() != 1 {
            return Err(CoreError::native_contract(
                "balanceOf expects exactly one argument".to_string(),
            ));
        }
        if args[0].len() != 20 {
            return Err(CoreError::native_contract(
                "Account argument must be 20 bytes".to_string(),
            ));
        }
        let account = UInt160::from_bytes(&args[0])
            .map_err(|err| CoreError::native_contract(err.to_string()))?;
        let snapshot = engine.snapshot_cache();
        let balance = self.balance_of_snapshot(snapshot.as_ref(), &account);
        Ok(Self::encode_amount(&balance))
    }

    fn transfer(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        // Enter reentrancy guard
        let _guard = SecurityContext::enter_guard(ReentrancyGuardType::GasTransfer)?;

        if args.len() != 4 {
            return Err(CoreError::native_contract(
                "transfer expects from, to, amount, data".to_string(),
            ));
        }
        let from = self.read_account(&args[0])?;
        let to = self.read_account(&args[1])?;
        let amount = Self::decode_amount(&args[2]);
        let data_bytes = args[3].clone();
        let data_item = if data_bytes.is_empty() {
            StackItem::null()
        } else {
            BinarySerializer::deserialize(&data_bytes, &ExecutionEngineLimits::default(), None)
                .unwrap_or_else(|_| StackItem::from_byte_string(data_bytes))
        };

        // Validate amount is non-negative
        PermissionValidator::validate_non_negative(&amount, "Transfer amount")?;

        // C# parity: FungibleToken.Transfer uses engine.CallingScriptHash, NOT
        // CurrentScriptHash.  CallingScriptHash is the contract that invoked
        // this native method; CurrentScriptHash is the native contract itself.
        let caller = engine.calling_script_hash();
        let watched_from = Self::is_watched_account(&from);
        let watched_to = Self::is_watched_account(&to);
        let from_matches_caller = from == caller;
        let witness_verified = if from_matches_caller {
            true
        } else {
            engine.check_witness_hash(&from)?
        };

        if watched_from || watched_to {
            tracing::info!(
                target: "neo",
                block_index = engine.current_block_index(),
                trigger = ?engine.trigger(),
                tx_hash = %Self::watched_tx_hash(engine),
                from = %from,
                to = %to,
                caller = %caller,
                amount = %amount,
                from_matches_caller,
                witness_verified,
                "watched GAS transfer authorization"
            );
        }

        if !witness_verified {
            if watched_from || watched_to {
                tracing::info!(
                    target: "neo",
                    block_index = engine.current_block_index(),
                    trigger = ?engine.trigger(),
                    tx_hash = %Self::watched_tx_hash(engine),
                    from = %from,
                    to = %to,
                    caller = %caller,
                    amount = %amount,
                    "watched GAS transfer rejected by witness check"
                );
            }
            return Ok(vec![0]);
        }

        if amount.is_zero() {
            self.emit_transfer_event(engine, Some(&from), Some(&to), &amount)?;
            let snapshot = engine.snapshot_cache();
            if ContractManagement::get_contract_from_snapshot(snapshot.as_ref(), &to)
                .map_err(|e| CoreError::native_contract(e.to_string()))?
                .is_some()
            {
                engine.queue_contract_call_from_native(
                    self.hash(),
                    to,
                    "onNEP17Payment",
                    vec![
                        StackItem::from_byte_string(from.to_bytes()),
                        StackItem::from_int(amount.clone()),
                        data_item,
                    ],
                );
            }
            return Ok(vec![1]);
        }

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let from_balance = self.balance_of_snapshot(snapshot_ref, &from);

        // Validate balance is sufficient
        if from_balance < amount {
            if watched_from || watched_to {
                tracing::info!(
                    target: "neo",
                    block_index = engine.current_block_index(),
                    trigger = ?engine.trigger(),
                    tx_hash = %Self::watched_tx_hash(engine),
                    from = %from,
                    to = %to,
                    caller = %caller,
                    amount = %amount,
                    from_balance = %from_balance,
                    "watched GAS transfer rejected by insufficient balance"
                );
            }
            return Ok(vec![0]);
        }

        // Use safe arithmetic for balance updates
        let new_from_balance = SafeArithmetic::safe_sub(&from_balance, &amount)?;
        let to_balance = self.balance_of_snapshot(snapshot_ref, &to);
        let new_to_balance = SafeArithmetic::safe_add(&to_balance, &amount)?;

        if from == to {
            self.emit_transfer_event(engine, Some(&from), Some(&to), &amount)?;
            if ContractManagement::get_contract_from_snapshot(snapshot_ref, &to)
                .map_err(|e| CoreError::native_contract(e.to_string()))?
                .is_some()
            {
                engine.queue_contract_call_from_native(
                    self.hash(),
                    to,
                    "onNEP17Payment",
                    vec![
                        StackItem::from_byte_string(from.to_bytes()),
                        StackItem::from_int(amount.clone()),
                        data_item,
                    ],
                );
            }
            return Ok(vec![1]);
        }

        let context = engine.get_native_storage_context(&self.hash())?;
        self.write_account_balance(&context, engine, &from, &new_from_balance)?;
        self.write_account_balance(&context, engine, &to, &new_to_balance)?;
        self.emit_transfer_event(engine, Some(&from), Some(&to), &amount)?;

        // Validate state consistency after transfer
        let final_from_balance = self.balance_of_snapshot(snapshot_ref, &from);
        let final_to_balance = self.balance_of_snapshot(snapshot_ref, &to);
        StateValidator::validate_account_state(&final_from_balance, 0, u32::MAX)?;
        StateValidator::validate_account_state(&final_to_balance, 0, u32::MAX)?;

        if ContractManagement::get_contract_from_snapshot(snapshot_ref, &to)
            .map_err(|e| CoreError::native_contract(e.to_string()))?
            .is_some()
        {
            engine.queue_contract_call_from_native(
                self.hash(),
                to,
                "onNEP17Payment",
                vec![
                    StackItem::from_byte_string(from.to_bytes()),
                    StackItem::from_int(amount.clone()),
                    data_item,
                ],
            );
        }

        if watched_from || watched_to {
            tracing::info!(
                target: "neo",
                block_index = engine.current_block_index(),
                trigger = ?engine.trigger(),
                tx_hash = %Self::watched_tx_hash(engine),
                from = %from,
                to = %to,
                caller = %caller,
                amount = %amount,
                from_balance_before = %from_balance,
                from_balance_after = %final_from_balance,
                to_balance_before = %to_balance,
                to_balance_after = %final_to_balance,
                "watched GAS transfer applied"
            );
        }

        Ok(vec![1])
    }

    fn read_account(&self, data: &[u8]) -> CoreResult<UInt160> {
        if data.len() != 20 {
            return Err(CoreError::native_contract(
                "Account argument must be 20 bytes".to_string(),
            ));
        }
        UInt160::from_bytes(data).map_err(|err| CoreError::native_contract(err.to_string()))
    }

    fn total_supply_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_TOTAL_SUPPLY)
    }

    fn total_supply_suffix() -> Vec<u8> {
        Self::total_supply_key().suffix().to_vec()
    }

    fn account_suffix(account: &UInt160) -> Vec<u8> {
        StorageKey::create_with_uint160(Self::ID, ACCOUNT_PREFIX, account)
            .suffix()
            .to_vec()
    }

    fn account_state_from_item(item: &StorageItem) -> AccountState {
        let bytes = item.value_bytes();
        if bytes.is_empty() {
            return AccountState::default();
        }

        if let Ok(stack_value @ StackValue::Struct(_)) =
            BinarySerializer::deserialize_stack_value(bytes.as_ref())
        {
            let mut state = AccountState::default();
            if let Err(e) = state.from_stack_value(stack_value) {
                tracing::warn!("Failed to deserialize AccountState from stack value: {e}");
                return AccountState::default();
            }
            return state;
        }

        AccountState::with_balance(item.to_bigint())
    }

    fn encode_amount(value: &BigInt) -> Vec<u8> {
        let mut bytes = value.to_signed_bytes_le();
        if bytes.is_empty() {
            bytes.push(0);
        }
        bytes
    }

    fn notary_fee_deduction(
        policy: &PolicyContract,
        snapshot: &crate::persistence::DataCache,
        tx: &Transaction,
    ) -> CoreResult<i64> {
        let Some(TransactionAttribute::NotaryAssisted(attr)) = tx
            .attributes()
            .iter()
            .find(|attribute| matches!(attribute, TransactionAttribute::NotaryAssisted(_)))
        else {
            return Ok(0);
        };

        let fee_per_key = policy
            .get_attribute_fee_for_type(snapshot, TransactionAttributeType::NotaryAssisted as u8)?;
        let nkeys = i64::from(attr.nkeys);
        nkeys
            .checked_add(1)
            .and_then(|n| n.checked_mul(fee_per_key))
            .ok_or_else(|| CoreError::native_contract("Notary fee calculation overflow"))
    }

    fn decode_amount(data: &[u8]) -> BigInt {
        BigInt::from_signed_bytes_le(data)
    }

    fn watched_account() -> Option<&'static UInt160> {
        static WATCHED: OnceLock<Option<UInt160>> = OnceLock::new();
        WATCHED
            .get_or_init(|| {
                let raw = std::env::var("NEO_GAS_WATCH_ACCOUNT").ok()?;
                let normalized = raw.trim();
                if normalized.is_empty() {
                    return None;
                }
                match UInt160::parse(normalized) {
                    Ok(value) => Some(value),
                    Err(err) => {
                        tracing::warn!(
                            target: "neo",
                            account = normalized,
                            error = %err,
                            "invalid NEO_GAS_WATCH_ACCOUNT; gas watch disabled"
                        );
                        None
                    }
                }
            })
            .as_ref()
    }

    fn is_watched_account(account: &UInt160) -> bool {
        Self::watched_account()
            .map(|watched| watched == account)
            .unwrap_or(false)
    }

    fn watched_tx_hash(engine: &ApplicationEngine) -> String {
        engine
            .script_container()
            .and_then(|container| container.hash().ok())
            .map(|hash| hash.to_string())
            .unwrap_or_else(|| "<none>".to_string())
    }

    fn write_account_balance(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        balance: &BigInt,
    ) -> CoreResult<()> {
        let watched = Self::is_watched_account(account);
        let previous_balance = if watched {
            let snapshot = engine.snapshot_cache();
            self.balance_of_snapshot(snapshot.as_ref(), account)
        } else {
            BigInt::zero()
        };
        let key = Self::account_suffix(account);
        if balance.is_zero() {
            engine.delete_storage_item(context, &key)?;
            if watched {
                tracing::info!(
                    target: "neo",
                    block_index = engine.current_block_index(),
                    trigger = ?engine.trigger(),
                    tx_hash = %Self::watched_tx_hash(engine),
                    account = %account,
                    previous_balance = %previous_balance,
                    next_balance = %balance,
                    op = "delete",
                    "watched GAS account balance change"
                );
            }
        } else {
            let state = AccountState::with_balance(balance.clone());
            let bytes = BinarySerializer::serialize_stack_value(
                &state.to_stack_value(),
                &ExecutionEngineLimits::default(),
            )
            .map_err(CoreError::native_contract)?;
            engine.put_storage_item(context, &key, &bytes)?;
            if watched {
                tracing::info!(
                    target: "neo",
                    block_index = engine.current_block_index(),
                    trigger = ?engine.trigger(),
                    tx_hash = %Self::watched_tx_hash(engine),
                    account = %account,
                    previous_balance = %previous_balance,
                    next_balance = %balance,
                    op = "put",
                    "watched GAS account balance change"
                );
            }
        }
        Ok(())
    }

    fn adjust_total_supply(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        delta: &BigInt,
    ) -> CoreResult<BigInt> {
        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let current = self.total_supply_snapshot(snapshot_ref);
        let updated = current + delta;
        if updated.is_negative() {
            return Err(CoreError::native_contract(
                "Total supply cannot be negative".to_string(),
            ));
        }

        let key = Self::total_supply_suffix();
        if updated.is_zero() {
            engine.delete_storage_item(context, &key)?;
        } else {
            let bytes = Self::encode_amount(&updated);
            engine.put_storage_item(context, &key, &bytes)?;
        }
        Ok(updated)
    }

    fn emit_transfer_event(
        &self,
        engine: &mut ApplicationEngine,
        from: Option<&UInt160>,
        to: Option<&UInt160>,
        amount: &BigInt,
    ) -> CoreResult<()> {
        // Use StackItem types matching C# FungibleToken.PostTransferAsync
        let from_item = from
            .map(|addr| StackItem::from_byte_string(addr.to_bytes()))
            .unwrap_or_else(StackItem::null);
        let to_item = to
            .map(|addr| StackItem::from_byte_string(addr.to_bytes()))
            .unwrap_or_else(StackItem::null);
        let amount_item = StackItem::from_int(amount.clone());

        // Use send_notification with explicit contract hash (like C# engine.SendNotification)
        engine
            .send_notification(
                self.hash(),
                "Transfer".to_string(),
                vec![from_item, to_item, amount_item],
            )
            .map_err(CoreError::native_contract)
    }

    /// Mints new GAS to the specified account.
    pub fn mint(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        amount: &BigInt,
        call_on_payment: bool,
    ) -> CoreResult<()> {
        // Enter reentrancy guard
        let _guard = SecurityContext::enter_guard(ReentrancyGuardType::GasMint)?;

        if amount.is_zero() {
            return Ok(());
        }

        // Validate amount is non-negative
        PermissionValidator::validate_non_negative(amount, "Mint amount")?;

        let context = engine.get_native_storage_context(&self.hash())?;
        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let balance = self.balance_of_snapshot(snapshot_ref, account);
        let watched = Self::is_watched_account(account);
        if watched {
            tracing::info!(
                target: "neo",
                block_index = engine.current_block_index(),
                trigger = ?engine.trigger(),
                tx_hash = %Self::watched_tx_hash(engine),
                account = %account,
                amount = %amount,
                balance_before = %balance,
                "watched GAS mint"
            );
        }

        // Use safe arithmetic
        let new_balance = SafeArithmetic::safe_add(&balance, amount)?;
        self.write_account_balance(&context, engine, account, &new_balance)?;
        self.adjust_total_supply(&context, engine, amount)?;
        self.emit_transfer_event(engine, None, Some(account), amount)?;

        // Validate state consistency
        StateValidator::validate_account_state(&new_balance, 0, u32::MAX)?;

        if call_on_payment {
            let snapshot = engine.snapshot_cache();
            if ContractManagement::get_contract_from_snapshot(snapshot.as_ref(), account)
                .map_err(|e| CoreError::native_contract(e.to_string()))?
                .is_some()
            {
                engine.queue_contract_call_from_native(
                    self.hash(),
                    *account,
                    "onNEP17Payment",
                    vec![
                        StackItem::null(),
                        StackItem::from_int(amount.clone()),
                        StackItem::null(),
                    ],
                );
            }
        }
        Ok(())
    }

    /// Burns GAS from the specified account.
    pub fn burn(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        amount: &BigInt,
    ) -> CoreResult<()> {
        // Enter reentrancy guard
        let _guard = SecurityContext::enter_guard(ReentrancyGuardType::GasBurn)?;

        if amount.is_zero() {
            return Ok(());
        }

        // Validate amount is non-negative
        PermissionValidator::validate_non_negative(amount, "Burn amount")?;

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let balance = self.balance_of_snapshot(snapshot_ref, account);
        let watched = Self::is_watched_account(account);
        if watched {
            tracing::info!(
                target: "neo",
                block_index = engine.current_block_index(),
                trigger = ?engine.trigger(),
                tx_hash = %Self::watched_tx_hash(engine),
                account = %account,
                amount = %amount,
                balance_before = %balance,
                "watched GAS burn"
            );
        }

        // Validate balance is sufficient
        if balance < *amount {
            return Err(CoreError::native_contract(
                "Insufficient balance for burn".to_string(),
            ));
        }

        // Use safe arithmetic
        let new_balance = SafeArithmetic::safe_sub(&balance, amount)?;

        let context = engine.get_native_storage_context(&self.hash())?;
        self.write_account_balance(&context, engine, account, &new_balance)?;
        let negative = -amount;
        self.adjust_total_supply(&context, engine, &negative)?;
        self.emit_transfer_event(engine, Some(account), None, amount)?;

        // Validate state consistency
        StateValidator::validate_account_state(&new_balance, 0, u32::MAX)?;

        Ok(())
    }

    /// Gets total supply from a snapshot (used by RPC/tests).
    pub fn total_supply_snapshot<S>(&self, snapshot: &S) -> BigInt
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = Self::total_supply_key();
        snapshot
            .try_get(&key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(BigInt::zero)
    }

    /// Reads the balance of `account` from the snapshot.
    pub fn balance_of_snapshot<S>(&self, snapshot: &S, account: &UInt160) -> BigInt
    where
        S: ReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create_with_uint160(Self::ID, ACCOUNT_PREFIX, account);
        snapshot
            .try_get(&key)
            .map(|item| Self::account_state_from_item(&item).balance)
            .unwrap_or_else(BigInt::zero)
    }
}

impl FungibleToken for GasToken {
    fn ft_symbol(&self) -> &str {
        Self::SYMBOL
    }

    fn ft_decimals(&self) -> u8 {
        Self::DECIMALS
    }

    fn ft_total_supply(&self, engine: &ApplicationEngine) -> CoreResult<BigInt> {
        let snapshot = engine.snapshot_cache();
        Ok(self.total_supply_snapshot(snapshot.as_ref()))
    }

    fn ft_balance_of(&self, engine: &ApplicationEngine, account: &UInt160) -> CoreResult<BigInt> {
        let snapshot = engine.snapshot_cache();
        Ok(self.balance_of_snapshot(snapshot.as_ref(), account))
    }
}

mod metadata;
mod native_impl;
