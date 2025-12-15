//! Notary native contract implementation.
//!
//! Mirrors the behaviour of `Neo.SmartContract.Native.Notary` exactly.
//! This contract assists with multisignature transaction forming by managing
//! GAS deposits for notary service fees.

use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::network::p2p::payloads::{Transaction, TransactionAttributeType};
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::native::helpers::NativeHelpers;
use crate::smart_contract::native::{
    gas_token::GasToken, ledger_contract::LedgerContract, policy_contract::PolicyContract,
    NativeContract, NativeMethod,
};
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::StorageItem;
use crate::UInt160;
use neo_vm::{ExecutionEngineLimits, StackItem};
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::any::Any;
use std::sync::Arc;

/// Storage key prefixes matching C# Notary contract.
const PREFIX_DEPOSIT: u8 = 1;
const PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;

/// Default maximum NotValidBefore delta (20 rounds for 7 validators).
const DEFAULT_MAX_NOT_VALID_BEFORE_DELTA: u32 = 140;
/// Default expiration delta applied when deposit owner cannot override the lock height.
const DEFAULT_DEPOSIT_DELTA_TILL: u32 = 5760;
/// Minimum number of blocks ahead of the current height a deposit must remain locked.
const MIN_DEPOSIT_LEAD: u32 = 2;

/// Notary deposit state (matches C# Deposit in Notary).
#[derive(Clone, Debug, Default)]
pub struct Deposit {
    /// The amount of GAS deposited.
    pub amount: BigInt,
    /// The block height until which the deposit is valid.
    pub till: u32,
}

impl Deposit {
    /// Creates a new deposit state.
    pub fn new(amount: BigInt, till: u32) -> Self {
        Self { amount, till }
    }
}

impl IInteroperable for Deposit {
    fn from_stack_item(&mut self, stack_item: StackItem) {
        if let StackItem::Struct(struct_item) = stack_item {
            let items = struct_item.items();
            if items.len() < 2 {
                return;
            }

            if let Ok(integer) = items[0].as_int() {
                self.amount = integer;
            }

            if let Ok(integer) = items[1].as_int() {
                if let Some(till) = integer.to_u32() {
                    self.till = till;
                }
            }
        }
    }

    fn to_stack_item(&self) -> StackItem {
        StackItem::from_struct(vec![
            StackItem::from_int(self.amount.clone()),
            StackItem::from_int(self.till),
        ])
    }

    fn clone_box(&self) -> Box<dyn IInteroperable> {
        Box::new(self.clone())
    }
}

/// Serializes a Deposit to bytes (matching C# StorageItem format).
fn serialize_deposit(deposit: &Deposit) -> Vec<u8> {
    let mut result = Vec::new();
    let amount_bytes = deposit.amount.to_signed_bytes_le();
    result.push(amount_bytes.len() as u8);
    result.extend_from_slice(&amount_bytes);
    result.extend_from_slice(&deposit.till.to_le_bytes());
    result
}

/// Deserializes a Deposit from bytes.
fn deserialize_deposit(data: &[u8]) -> Result<Deposit> {
    if data.is_empty() {
        return Err(Error::native_contract("Empty deposit data".to_string()));
    }
    let amount_len = data[0] as usize;
    if data.len() < 1 + amount_len + 4 {
        return Err(Error::native_contract(
            "Invalid deposit data length".to_string(),
        ));
    }
    let amount_bytes = &data[1..1 + amount_len];
    let amount = BigInt::from_signed_bytes_le(amount_bytes);
    let till_bytes = &data[1 + amount_len..1 + amount_len + 4];
    let till = u32::from_le_bytes([till_bytes[0], till_bytes[1], till_bytes[2], till_bytes[3]]);
    Ok(Deposit::new(amount, till))
}

/// The Notary native contract.
///
/// Activates with HF_Echidna hardfork.
pub struct Notary {
    /// Contract ID (fixed at -10 per Neo protocol).
    id: i32,
    /// Contract script hash.
    hash: UInt160,
    /// Registered methods.
    methods: Vec<NativeMethod>,
}

impl Default for Notary {
    fn default() -> Self {
        Self::new()
    }
}

impl Notary {
    /// Contract ID for Notary (-10 per Neo protocol).
    pub const ID: i32 = -10;

    /// Creates a new Notary contract instance.
    pub fn new() -> Self {
        // Notary contract hash: 0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b
        // This is the official Neo N3 Notary contract hash computed from:
        // Helper.GetContractHash(UInt160.Zero, 0, "Notary")
        let hash = UInt160::from_bytes(&[
            0xc1, 0xe1, 0x4f, 0x19, 0xc3, 0xe6, 0x0d, 0x0b, 0x92, 0x44, 0xd0, 0x6d, 0xd7, 0xba,
            0x9b, 0x11, 0x31, 0x35, 0xec, 0x3b,
        ])
        .expect("Valid Notary contract hash");

        let methods = vec![
            // Query methods
            NativeMethod::safe(
                "balanceOf".to_string(),
                1 << 15,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "expirationOf".to_string(),
                1 << 15,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Integer,
            ),
            NativeMethod::safe(
                "getMaxNotValidBeforeDelta".to_string(),
                1 << 15,
                Vec::new(),
                ContractParameterType::Integer,
            ),
            // Deposit management methods (write operations)
            NativeMethod::unsafe_method(
                "onNEP17Payment".to_string(),
                1 << 15,
                0x00,
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                    ContractParameterType::Any,
                ],
                ContractParameterType::Void,
            ),
            NativeMethod::unsafe_method(
                "lockDepositUntil".to_string(),
                1 << 15,
                0x00,
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Boolean,
            ),
            NativeMethod::unsafe_method(
                "withdraw".to_string(),
                1 << 15,
                0x00,
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                ],
                ContractParameterType::Boolean,
            ),
            NativeMethod::unsafe_method(
                "setMaxNotValidBeforeDelta".to_string(),
                1 << 15,
                0x00,
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            ),
        ];

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }

    /// Gets storage key for a deposit.
    fn deposit_key(account: &UInt160) -> StorageKey {
        let mut key = vec![PREFIX_DEPOSIT];
        key.extend_from_slice(&account.to_bytes());
        StorageKey::from_bytes(&key)
    }

    /// Gets storage key for max not valid before delta.
    fn max_delta_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_MAX_NOT_VALID_BEFORE_DELTA)
    }

    fn parse_deposit_metadata(
        data: Option<&Vec<u8>>,
        default_owner: &UInt160,
    ) -> Result<(UInt160, Option<u32>)> {
        match data {
            None => Ok((*default_owner, None)),
            Some(bytes) if bytes.is_empty() => Ok((*default_owner, None)),
            Some(bytes) if bytes.len() == 4 => {
                let mut till_bytes = [0u8; 4];
                till_bytes.copy_from_slice(bytes);
                let till = u32::from_le_bytes(till_bytes);
                Ok((*default_owner, Some(till)))
            }
            Some(bytes) if bytes.len() == UInt160::LENGTH => {
                let owner = UInt160::from_bytes(bytes)
                    .map_err(|_| Error::native_contract("Invalid deposit recipient".to_string()))?;
                Ok((owner, None))
            }
            Some(bytes) if bytes.len() == UInt160::LENGTH + 4 => {
                let (owner_bytes, till_bytes) = bytes.split_at(UInt160::LENGTH);
                let owner = UInt160::from_bytes(owner_bytes)
                    .map_err(|_| Error::native_contract("Invalid deposit recipient".to_string()))?;
                let till = u32::from_le_bytes(<[u8; 4]>::try_from(till_bytes).map_err(|_| {
                    Error::native_contract("Failed to parse deposit expiration".to_string())
                })?);
                Ok((owner, Some(till)))
            }
            Some(bytes) => {
                if let Ok(item) =
                    BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None)
                {
                    Self::parse_stack_metadata(item, default_owner)
                } else {
                    Err(Error::native_contract(
                        "Deposit metadata must be 0, 4, 20, 24 bytes, or serialized array"
                            .to_string(),
                    ))
                }
            }
        }
    }

    fn parse_stack_metadata(
        item: StackItem,
        default_owner: &UInt160,
    ) -> Result<(UInt160, Option<u32>)> {
        let (owner_item, till_item) = match item {
            StackItem::Array(array) => {
                let items = array.items();
                if items.len() != 2 {
                    return Err(Error::native_contract(
                        "Deposit metadata array must have exactly two elements".to_string(),
                    ));
                }
                (items[0].clone(), items[1].clone())
            }
            StackItem::Struct(struct_item) => {
                let items = struct_item.items();
                if items.len() != 2 {
                    return Err(Error::native_contract(
                        "Deposit metadata struct must have exactly two elements".to_string(),
                    ));
                }
                (items[0].clone(), items[1].clone())
            }
            _other => {
                return Err(Error::native_contract(
                    "Unsupported deposit metadata type".to_string(),
                ))
            }
        };

        let owner = if owner_item.is_null() {
            *default_owner
        } else {
            let bytes = owner_item
                .as_bytes()
                .map_err(|err| Error::native_contract(format!("Invalid deposit owner: {}", err)))?;
            if bytes.len() != UInt160::LENGTH {
                return Err(Error::native_contract(
                    "Deposit owner must be 20 bytes".to_string(),
                ));
            }
            UInt160::from_bytes(&bytes)
                .map_err(|_| Error::native_contract("Invalid deposit recipient".to_string()))?
        };

        let till = if till_item.is_null() {
            None
        } else {
            let value = till_item.get_integer().map_err(|err| {
                Error::native_contract(format!("Invalid deposit expiration: {}", err))
            })?;
            value
                .to_u32()
                .ok_or_else(|| {
                    Error::native_contract("Deposit expiration must fit in u32".to_string())
                })
                .map(Some)?
        };

        Ok((owner, till))
    }

    /// Gets the GAS balance of an account (Arc version for engine calls).
    pub fn balance_of_arc(&self, snapshot: &Arc<DataCache>, account: &UInt160) -> BigInt {
        self.balance_of(snapshot.as_ref(), account)
    }

    /// Gets the GAS balance of an account.
    pub fn balance_of(&self, snapshot: &DataCache, account: &UInt160) -> BigInt {
        let key = Self::deposit_key(account);
        match snapshot.try_get(&key) {
            Some(item) => {
                let data = item.get_value();
                match deserialize_deposit(&data) {
                    Ok(deposit) => deposit.amount,
                    Err(_) => BigInt::zero(),
                }
            }
            None => BigInt::zero(),
        }
    }

    /// Gets the expiration block of an account's deposit (Arc version for engine calls).
    pub fn expiration_of_arc(&self, snapshot: &Arc<DataCache>, account: &UInt160) -> u32 {
        self.expiration_of(snapshot.as_ref(), account)
    }

    /// Gets the expiration block of an account's deposit.
    pub fn expiration_of(&self, snapshot: &DataCache, account: &UInt160) -> u32 {
        let key = Self::deposit_key(account);
        match snapshot.try_get(&key) {
            Some(item) => {
                let data = item.get_value();
                match deserialize_deposit(&data) {
                    Ok(deposit) => deposit.till,
                    Err(_) => 0,
                }
            }
            None => 0,
        }
    }

    /// Gets the maximum NotValidBefore delta (Arc version for engine calls).
    pub fn get_max_not_valid_before_delta_arc(&self, snapshot: &Arc<DataCache>) -> u32 {
        self.get_max_not_valid_before_delta(snapshot.as_ref())
    }

    /// Gets the maximum NotValidBefore delta.
    pub fn get_max_not_valid_before_delta(&self, snapshot: &DataCache) -> u32 {
        let key = Self::max_delta_key();
        match snapshot.try_get(&key) {
            Some(item) => {
                let data = item.get_value();
                if data.len() >= 4 {
                    u32::from_le_bytes([data[0], data[1], data[2], data[3]])
                } else {
                    DEFAULT_MAX_NOT_VALID_BEFORE_DELTA
                }
            }
            None => DEFAULT_MAX_NOT_VALID_BEFORE_DELTA,
        }
    }

    /// Sets deposit for an account.
    #[allow(dead_code)]
    pub fn set_deposit(&self, snapshot: &DataCache, account: &UInt160, deposit: &Deposit) {
        let key = Self::deposit_key(account);
        let data = serialize_deposit(deposit);
        snapshot.add(key, StorageItem::from_bytes(data));
    }

    fn persist_deposit(
        snapshot: &Arc<DataCache>,
        key: StorageKey,
        existed: bool,
        deposit: &Deposit,
    ) {
        if deposit.amount.is_zero() {
            snapshot.delete(&key);
        } else {
            let data = StorageItem::from_bytes(serialize_deposit(deposit));
            if existed {
                snapshot.update(key, data);
            } else {
                snapshot.add(key, data);
            }
        }
    }

    /// Handle NEP-17 GAS payment (deposit)
    fn on_nep17_payment(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        // Verify caller is GAS contract
        let caller = engine.calling_script_hash();
        let gas_hash = Helper::get_contract_hash(&UInt160::zero(), 0, "GasToken");
        if caller != gas_hash {
            return Err(Error::native_contract(
                "onNEP17Payment can only be called by GAS contract".to_string(),
            ));
        }

        if args.len() < 2 {
            return Err(Error::native_contract(
                "onNEP17Payment requires from and amount arguments".to_string(),
            ));
        }

        let from = UInt160::from_bytes(&args[0])
            .map_err(|_| Error::native_contract("Invalid from address".to_string()))?;
        let amount = BigInt::from_signed_bytes_le(&args[1]);

        if amount.is_negative() || amount.is_zero() {
            return Err(Error::native_contract(
                "Deposit amount must be positive".to_string(),
            ));
        }

        let snapshot = engine.snapshot_cache();
        let ledger = LedgerContract::new();
        let current_height = ledger
            .current_index(snapshot.as_ref())
            .unwrap_or_else(|_| NativeHelpers::current_index());
        let max_delta = self.get_max_not_valid_before_delta_arc(&snapshot);

        let (deposit_owner, requested_till) = Self::parse_deposit_metadata(args.get(2), &from)?;

        let tx_sender = engine
            .script_container()
            .and_then(|container| container.as_transaction())
            .and_then(Transaction::sender);
        let allowed_change_till = tx_sender == Some(deposit_owner);

        let key = Self::deposit_key(&deposit_owner);
        let (mut deposit, has_existing_deposit) = match snapshot.as_ref().try_get(&key) {
            Some(item) => (deserialize_deposit(&item.get_value())?, true),
            None => (Deposit::new(BigInt::zero(), 0), false),
        };
        let previous_till = deposit.till;

        let mut effective_till = requested_till
            .or(if previous_till > 0 {
                Some(previous_till)
            } else {
                None
            })
            .unwrap_or_else(|| current_height.saturating_add(max_delta));

        if has_existing_deposit && requested_till.is_some() && effective_till < previous_till {
            return Err(Error::native_contract(
                "Deposit expiration cannot be reduced".to_string(),
            ));
        }

        if has_existing_deposit {
            if !allowed_change_till {
                effective_till = previous_till;
            }
        } else if !allowed_change_till {
            let enforced_delta = DEFAULT_DEPOSIT_DELTA_TILL.min(max_delta);
            effective_till = current_height.saturating_add(enforced_delta);
        }

        let min_allowed = current_height.saturating_add(MIN_DEPOSIT_LEAD);
        if effective_till < min_allowed {
            return Err(Error::native_contract(
                "Deposit expiration must be at least two blocks ahead of current height"
                    .to_string(),
            ));
        }

        let max_allowed = current_height.saturating_add(max_delta);
        if effective_till > max_allowed {
            return Err(Error::native_contract(
                "Deposit expiration exceeds maximum allowed delta".to_string(),
            ));
        }

        if !has_existing_deposit {
            let policy = PolicyContract::new();
            let fee_per_key = policy
                .get_attribute_fee_for_type(
                    snapshot.as_ref(),
                    TransactionAttributeType::NotaryAssisted as u8,
                )
                .map_err(|err| {
                    Error::native_contract(format!("Failed to read Notary attribute fee: {}", err))
                })?;
            let min_required = BigInt::from(2) * BigInt::from(fee_per_key);
            if amount < min_required {
                return Err(Error::native_contract(format!(
                    "Initial deposit must be at least {} datoshi of GAS",
                    min_required
                )));
            }
        }

        deposit.amount += amount;
        deposit.till = effective_till.max(previous_till);

        Self::persist_deposit(&snapshot, key, has_existing_deposit, &deposit);

        Ok(vec![1]) // true
    }

    /// Lock deposit until specified block
    fn lock_deposit_until(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "lockDepositUntil requires account and till arguments".to_string(),
            ));
        }

        let account = UInt160::from_bytes(&args[0])
            .map_err(|_| Error::native_contract("Invalid account address".to_string()))?;
        if args[1].len() < 4 {
            return Err(Error::native_contract("Invalid till argument".to_string()));
        }
        let till = u32::from_le_bytes([args[1][0], args[1][1], args[1][2], args[1][3]]);

        // Verify witness for account
        if !engine.check_witness_hash(&account)? {
            return Err(Error::native_contract("No witness for account".to_string()));
        }

        let snapshot = engine.snapshot_cache();
        let key = Self::deposit_key(&account);

        let current_deposit = match snapshot.as_ref().try_get(&key) {
            Some(item) => deserialize_deposit(&item.get_value())?,
            None => {
                return Err(Error::native_contract(
                    "No deposit found for account".to_string(),
                ));
            }
        };

        // Can only extend, not reduce lock period
        if till <= current_deposit.till {
            return Err(Error::native_contract(
                "Can only extend deposit lock, not reduce".to_string(),
            ));
        }

        let ledger = LedgerContract::new();
        let current_height = ledger
            .current_index(snapshot.as_ref())
            .unwrap_or_else(|_| NativeHelpers::current_index());
        if till < current_height.saturating_add(MIN_DEPOSIT_LEAD) {
            return Err(Error::native_contract(
                "Deposit expiration must be at least two blocks ahead of current height"
                    .to_string(),
            ));
        }
        let max_delta = self.get_max_not_valid_before_delta_arc(&snapshot);
        if till > current_height.saturating_add(max_delta) {
            return Err(Error::native_contract(
                "Till exceeds maximum allowed delta".to_string(),
            ));
        }

        let new_deposit = Deposit::new(current_deposit.amount, till);
        Self::persist_deposit(&snapshot, key, true, &new_deposit);

        Ok(vec![1]) // true
    }

    /// Withdraw deposit after expiration
    fn withdraw(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.len() < 2 {
            return Err(Error::native_contract(
                "withdraw requires from and to arguments".to_string(),
            ));
        }

        let from = UInt160::from_bytes(&args[0])
            .map_err(|_| Error::native_contract("Invalid from address".to_string()))?;
        let to = if args[1].is_empty() {
            from
        } else {
            UInt160::from_bytes(&args[1])
                .map_err(|_| Error::native_contract("Invalid to address".to_string()))?
        };

        // Verify witness for from account
        if !engine.check_witness_hash(&from)? {
            return Err(Error::native_contract(
                "No witness for from account".to_string(),
            ));
        }

        let snapshot = engine.snapshot_cache();
        let key = Self::deposit_key(&from);

        let deposit = match snapshot.as_ref().try_get(&key) {
            Some(item) => deserialize_deposit(&item.get_value())?,
            None => {
                return Err(Error::native_contract(
                    "No deposit found for account".to_string(),
                ));
            }
        };

        // Check if deposit has expired
        let current_height = NativeHelpers::current_index();
        if deposit.till > current_height {
            return Err(Error::native_contract(
                "Deposit has not expired yet".to_string(),
            ));
        }

        // Delete deposit
        snapshot.delete(&key);

        // Perform GAS transfer from Notary contract hash to destination
        let contract_hash = self.hash();
        let mut transfer_args = Vec::with_capacity(3);
        transfer_args.push(contract_hash.to_bytes().to_vec());
        transfer_args.push(to.to_bytes().to_vec());
        let mut amount_bytes = deposit.amount.to_signed_bytes_le();
        if amount_bytes.is_empty() {
            amount_bytes.push(0);
        }
        transfer_args.push(amount_bytes);
        // NEP-17 transfer requires a data argument; pass null.
        transfer_args.push(Vec::new());

        // Temporarily mark the current native caller so GAS transfer authorizes Notary
        let state_arc = engine.current_execution_state().map_err(|err| {
            Error::native_contract(format!(
                "Failed to access execution state for withdraw: {}",
                err
            ))
        })?;
        let (prev_native_caller, prev_flags) = {
            let mut state = state_arc.lock();
            let previous_caller = state.native_calling_script_hash;
            let previous_flags = state.call_flags;
            state.native_calling_script_hash = Some(contract_hash);
            state.call_flags = CallFlags::ALL;
            (previous_caller, previous_flags)
        };

        let call_result =
            engine.call_native_contract(GasToken::new().hash(), "transfer", &transfer_args);

        {
            let mut state = state_arc.lock();
            state.native_calling_script_hash = prev_native_caller;
            state.call_flags = prev_flags;
        }

        let transfer_result = call_result?;
        if transfer_result.first().copied() != Some(1) {
            return Err(Error::native_contract(
                "Failed to transfer GAS during withdraw".to_string(),
            ));
        }

        Ok(vec![1]) // true
    }

    /// Set maximum NotValidBefore delta (committee only)
    fn set_max_not_valid_before_delta(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        // Verify committee witness against current committee address.
        let snapshot = engine.snapshot_cache();
        let committee_address =
            NativeHelpers::committee_address(engine.protocol_settings(), Some(snapshot.as_ref()));
        if !engine.check_witness_hash(&committee_address)? {
            return Err(Error::native_contract(
                "setMaxNotValidBeforeDelta requires committee witness".to_string(),
            ));
        }

        if args.is_empty() || args[0].len() < 4 {
            return Err(Error::native_contract(
                "setMaxNotValidBeforeDelta requires value argument".to_string(),
            ));
        }

        let value = u32::from_le_bytes([args[0][0], args[0][1], args[0][2], args[0][3]]);
        if value == 0 {
            return Err(Error::native_contract(
                "Max delta must be positive".to_string(),
            ));
        }

        let snapshot = engine.snapshot_cache();
        let policy = PolicyContract::new();
        let max_valid_increment = policy
            .get_max_valid_until_block_increment_snapshot(
                snapshot.as_ref(),
                engine.protocol_settings(),
            )
            .map_err(|err| {
                Error::native_contract(format!(
                    "Failed to read MaxValidUntilBlock increment: {}",
                    err
                ))
            })?;
        let min_allowed = engine.protocol_settings().validators_count.max(0) as u32;
        let max_allowed = max_valid_increment.saturating_div(2);

        if value < min_allowed || value > max_allowed {
            return Err(Error::native_contract(format!(
                "Max delta must be between {} and {}",
                min_allowed, max_allowed
            )));
        }

        let key = Self::max_delta_key();
        snapshot.add(key, StorageItem::from_bytes(value.to_le_bytes().to_vec()));

        Ok(vec![])
    }
}

impl NativeContract for Notary {
    fn id(&self) -> i32 {
        self.id
    }

    fn name(&self) -> &str {
        "Notary"
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn is_active(&self, settings: &ProtocolSettings, block_height: u32) -> bool {
        settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height)
    }

    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        vec!["NEP-27".to_string()]
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "balanceOf" => {
                if args.is_empty() {
                    return Err(Error::native_contract(
                        "balanceOf requires account argument".to_string(),
                    ));
                }
                let account = UInt160::from_bytes(&args[0])
                    .map_err(|_| Error::native_contract("Invalid account hash".to_string()))?;
                let balance = self.balance_of_arc(&snapshot, &account);
                // Return as integer bytes
                Ok(balance.to_signed_bytes_le())
            }
            "expirationOf" => {
                if args.is_empty() {
                    return Err(Error::native_contract(
                        "expirationOf requires account argument".to_string(),
                    ));
                }
                let account = UInt160::from_bytes(&args[0])
                    .map_err(|_| Error::native_contract("Invalid account hash".to_string()))?;
                let expiration = self.expiration_of_arc(&snapshot, &account);
                Ok(expiration.to_le_bytes().to_vec())
            }
            "getMaxNotValidBeforeDelta" => {
                let delta = self.get_max_not_valid_before_delta_arc(&snapshot);
                Ok(delta.to_le_bytes().to_vec())
            }
            "onNEP17Payment" => {
                // Handle GAS deposits from users
                // Args: from (UInt160), amount (BigInt), data (optional)
                self.on_nep17_payment(engine, args)
            }
            "lockDepositUntil" => {
                // Extend deposit lock period
                // Args: account (UInt160), till (u32)
                self.lock_deposit_until(engine, args)
            }
            "withdraw" => {
                // Withdraw deposit after expiration
                // Args: from (UInt160), to (UInt160)
                self.withdraw(engine, args)
            }
            "setMaxNotValidBeforeDelta" => {
                // Set max delta (committee only)
                // Args: value (u32)
                self.set_max_not_valid_before_delta(engine, args)
            }
            _ => Err(Error::native_contract(format!(
                "Unknown Notary method: {}",
                method
            ))),
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notary_creation() {
        let notary = Notary::new();
        assert_eq!(notary.id(), Notary::ID);
        assert_eq!(notary.name(), "Notary");
    }

    #[test]
    fn test_deposit_serialization() {
        let deposit = Deposit::new(BigInt::from(1000000000i64), 12345);
        let data = serialize_deposit(&deposit);
        let deserialized = deserialize_deposit(&data).unwrap();
        assert_eq!(deserialized.amount, deposit.amount);
        assert_eq!(deserialized.till, deposit.till);
    }

    #[test]
    fn test_deposit_to_stack_item() {
        let deposit = Deposit::new(BigInt::from(500), 100);
        let item = deposit.to_stack_item();
        let mut new_deposit = Deposit::default();
        new_deposit.from_stack_item(item);
        assert_eq!(new_deposit.amount, deposit.amount);
        assert_eq!(new_deposit.till, deposit.till);
    }
}
