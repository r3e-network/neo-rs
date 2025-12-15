use super::contract_management::ContractManagement;
use super::fungible_token::{PREFIX_ACCOUNT as ACCOUNT_PREFIX, PREFIX_TOTAL_SUPPLY};
use super::helpers::NativeHelpers;
use super::native_contract::{NativeContract, NativeMethod};
use super::neo_token::NeoToken;
use super::policy_contract::PolicyContract;
use crate::error::{CoreError, CoreResult};
use crate::network::p2p::payloads::{Transaction, TransactionAttribute, TransactionAttributeType};
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::storage_context::StorageContext;
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::StorageItem;
use crate::UInt160;
use lazy_static::lazy_static;
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::{Signed, Zero};
use std::any::Any;

lazy_static! {
    static ref GAS_HASH: UInt160 = Helper::get_contract_hash(&UInt160::zero(), 0, "GasToken");
}

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
        let methods = vec![
            NativeMethod::safe(
                "symbol".to_string(),
                0,
                Vec::new(),
                ContractParameterType::String,
            ),
            NativeMethod::safe(
                "decimals".to_string(),
                0,
                Vec::new(),
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "totalSupply".to_string(),
                1 << 15,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(crate::smart_contract::call_flags::CallFlags::READ_STATES),
            NativeMethod::safe(
                "balanceOf".to_string(),
                1 << 15,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Integer,
            )
            .with_required_call_flags(crate::smart_contract::call_flags::CallFlags::READ_STATES)
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::unsafe_method(
                "transfer".to_string(),
                1 << 17,
                crate::smart_contract::call_flags::CallFlags::ALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Boolean,
            )
            .with_storage_fee(50)
            .with_parameter_names(vec![
                "from".to_string(),
                "to".to_string(),
                "amount".to_string(),
                "data".to_string(),
            ]),
        ];

        Self { methods }
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
                "Method not implemented: {}",
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
            StackItem::from_byte_string(data_bytes)
        };
        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "Amount cannot be negative".to_string(),
            ));
        }

        let caller = engine.calling_script_hash();
        if from != caller && !engine.check_witness_hash(&from)? {
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
        let mut from_balance = self.balance_of_snapshot(snapshot_ref, &from);
        if from_balance < amount {
            return Ok(vec![0]);
        }
        from_balance -= &amount;
        let mut to_balance = self.balance_of_snapshot(snapshot_ref, &to);
        to_balance += &amount;

        let context = engine.get_native_storage_context(&self.hash())?;
        self.write_account_balance(&context, engine, &from, &from_balance)?;
        self.write_account_balance(&context, engine, &to, &to_balance)?;
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
        Ok((attr.nkeys as i64 + 1) * fee_per_key)
    }

    fn decode_amount(data: &[u8]) -> BigInt {
        BigInt::from_signed_bytes_le(data)
    }

    fn write_account_balance(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        balance: &BigInt,
    ) -> CoreResult<()> {
        let key = Self::account_suffix(account);
        if balance.is_zero() {
            engine.delete_storage_item(context, &key)?;
        } else {
            let bytes = Self::encode_amount(balance);
            engine.put_storage_item(context, &key, &bytes)?;
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
        if amount.is_zero() {
            return Ok(());
        }
        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "Mint amount cannot be negative".to_string(),
            ));
        }

        let context = engine.get_native_storage_context(&self.hash())?;
        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let mut balance = self.balance_of_snapshot(snapshot_ref, account);
        balance += amount;
        self.write_account_balance(&context, engine, account, &balance)?;
        self.adjust_total_supply(&context, engine, amount)?;
        self.emit_transfer_event(engine, None, Some(account), amount)?;
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
        if amount.is_zero() {
            return Ok(());
        }
        if amount.is_negative() {
            return Err(CoreError::native_contract(
                "Burn amount cannot be negative".to_string(),
            ));
        }

        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let mut balance = self.balance_of_snapshot(snapshot_ref, account);
        if balance < *amount {
            return Err(CoreError::native_contract(
                "Insufficient balance for burn".to_string(),
            ));
        }
        balance -= amount;

        let context = engine.get_native_storage_context(&self.hash())?;
        self.write_account_balance(&context, engine, account, &balance)?;
        let negative = -amount;
        self.adjust_total_supply(&context, engine, &negative)?;
        self.emit_transfer_event(engine, Some(account), None, amount)?;
        Ok(())
    }

    /// Gets total supply from a snapshot (used by RPC/tests).
    pub fn total_supply_snapshot<S>(&self, snapshot: &S) -> BigInt
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
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
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = StorageKey::create_with_uint160(Self::ID, ACCOUNT_PREFIX, account);
        snapshot
            .try_get(&key)
            .map(|item| item.to_bigint())
            .unwrap_or_else(BigInt::zero)
    }
}

impl NativeContract for GasToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *GAS_HASH
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn is_active(&self, _settings: &ProtocolSettings, _block_height: u32) -> bool {
        true
    }

    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        vec!["NEP-17".to_string()]
    }

    fn events(
        &self,
        _settings: &ProtocolSettings,
        _block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        vec![ContractEventDescriptor::new(
            "Transfer".to_string(),
            vec![
                ContractParameterDefinition::new(
                    "from".to_string(),
                    ContractParameterType::Hash160,
                )
                .expect("Transfer.from"),
                ContractParameterDefinition::new("to".to_string(), ContractParameterType::Hash160)
                    .expect("Transfer.to"),
                ContractParameterDefinition::new(
                    "amount".to_string(),
                    ContractParameterType::Integer,
                )
                .expect("Transfer.amount"),
            ],
        )
        .expect("Transfer event descriptor")]
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        if snapshot
            .as_ref()
            .try_get(&Self::total_supply_key())
            .is_some()
        {
            return Ok(());
        }

        let validators = engine.protocol_settings().standby_validators();
        if validators.is_empty() {
            return Ok(());
        }
        let account = NativeHelpers::get_bft_address(&validators);
        let amount = BigInt::from(engine.protocol_settings().initial_gas_distribution);
        self.mint(engine, &account, &amount, false)
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_method(engine, method, args)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// OnPersist: Burns system+network fees from senders, mints network fees to primary validator.
    /// Matches C# GasToken.OnPersistAsync exactly.
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let block = engine.persisting_block().cloned().ok_or_else(|| {
            CoreError::native_contract("No persisting block available".to_string())
        })?;

        let mut total_network_fee: i64 = 0;
        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let policy = PolicyContract::new();

        // Burn system fee + network fee from each transaction sender
        for tx in &block.transactions {
            let sender = match tx.sender() {
                Some(s) => s,
                None => continue, // Skip transactions without a sender
            };
            let total_fee = tx.system_fee() + tx.network_fee();
            let burn_amount = BigInt::from(total_fee);
            self.burn(engine, &sender, &burn_amount)?;
            total_network_fee += tx.network_fee();

            total_network_fee -= Self::notary_fee_deduction(&policy, snapshot_ref, tx)?;
        }

        // Mint total network fee to the primary consensus node
        if total_network_fee > 0 {
            let neo_token = NeoToken::new();
            let validators = neo_token
                .get_next_block_validators_snapshot(
                    snapshot_ref,
                    engine.protocol_settings().validators_count as usize,
                    engine.protocol_settings(),
                )
                .unwrap_or_else(|_| engine.protocol_settings().standby_validators());

            if !validators.is_empty() {
                let primary_index = block.header.primary_index as usize;
                if primary_index < validators.len() {
                    let primary_validator = &validators[primary_index];
                    let primary_account =
                        crate::smart_contract::Contract::create_signature_contract(
                            primary_validator.clone(),
                        )
                        .script_hash();
                    let mint_amount = BigInt::from(total_network_fee);
                    self.mint(engine, &primary_account, &mint_amount, false)?;
                }
            }
        }

        Ok(())
    }
}
