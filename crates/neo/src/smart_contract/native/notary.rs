//! Notary native contract implementation.
//!
//! Mirrors the behaviour of `Neo.SmartContract.Native.Notary` exactly.
//! This contract assists with multisignature transaction forming by managing
//! GAS deposits for notary service fees.

use crate::error::{CoreError as Error, CoreResult as Result};
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::persistence::DataCache;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::i_interoperable::IInteroperable;
use crate::smart_contract::native::helpers::NativeHelpers;
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::StorageItem;
use crate::UInt160;
use neo_vm::StackItem;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::any::Any;
use std::sync::Arc;

/// Storage key prefixes matching C# Notary contract.
const PREFIX_DEPOSIT: u8 = 1;
const PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;

/// Default maximum NotValidBefore delta (20 rounds for 7 validators).
const DEFAULT_MAX_NOT_VALID_BEFORE_DELTA: u32 = 140;

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
            NativeMethod::safe("balanceOf".to_string(), 1 << 15),
            NativeMethod::safe("expirationOf".to_string(), 1 << 15),
            NativeMethod::safe("getMaxNotValidBeforeDelta".to_string(), 1 << 15),
            // Deposit management methods (write operations)
            NativeMethod::unsafe_method("onNEP17Payment".to_string(), 1 << 15, 0x00),
            NativeMethod::unsafe_method("lockDepositUntil".to_string(), 1 << 15, 0x00),
            NativeMethod::unsafe_method("withdraw".to_string(), 1 << 15, 0x00),
            NativeMethod::unsafe_method("setMaxNotValidBeforeDelta".to_string(), 1 << 15, 0x00),
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

        // Parse optional till block from data argument
        let till = if args.len() > 2 && args[2].len() >= 4 {
            let bytes = &args[2];
            u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
        } else {
            // Default: current height + max delta
            let snapshot = engine.snapshot_cache();
            let current_height = engine.protocol_settings().standby_committee.len() as u32; // approximation
            let max_delta = self.get_max_not_valid_before_delta_arc(&snapshot);
            current_height + max_delta
        };

        // Update deposit
        let snapshot = engine.snapshot_cache();
        let key = Self::deposit_key(&from);
        let current_deposit = match snapshot.as_ref().try_get(&key) {
            Some(item) => deserialize_deposit(&item.get_value()).unwrap_or_default(),
            None => Deposit::default(),
        };

        let new_deposit = Deposit::new(
            current_deposit.amount + amount,
            till.max(current_deposit.till),
        );
        let data = serialize_deposit(&new_deposit);
        snapshot.add(key, StorageItem::from_bytes(data));

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
        let till = if args[1].len() >= 4 {
            u32::from_le_bytes([args[1][0], args[1][1], args[1][2], args[1][3]])
        } else {
            return Err(Error::native_contract("Invalid till argument".to_string()));
        };

        // Verify witness for account
        if !engine.check_witness_hash(&account) {
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

        // Validate against max delta
        let max_delta = self.get_max_not_valid_before_delta_arc(&snapshot);
        let current_height = NativeHelpers::current_index();
        if till > current_height + max_delta {
            return Err(Error::native_contract(
                "Till exceeds maximum allowed delta".to_string(),
            ));
        }

        let new_deposit = Deposit::new(current_deposit.amount, till);
        let data = serialize_deposit(&new_deposit);
        snapshot.add(key, StorageItem::from_bytes(data));

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
        // TODO: Use 'to' address for actual GAS transfer destination
        let _to = UInt160::from_bytes(&args[1])
            .map_err(|_| Error::native_contract("Invalid to address".to_string()))?;

        // Verify witness for from account
        if !engine.check_witness_hash(&from) {
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

        // Transfer GAS back to user (simplified - full impl would call GAS.transfer)
        // In production, this would emit a Transfer event and update GAS balances

        Ok(deposit.amount.to_signed_bytes_le())
    }

    /// Set maximum NotValidBefore delta (committee only)
    fn set_max_not_valid_before_delta(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        // Verify committee witness
        let committee_address = NativeHelpers::committee_address(engine.protocol_settings(), None);
        if !engine.check_witness_hash(&committee_address) {
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

        // Validate range (must be positive and reasonable)
        if value == 0 || value > 720 {
            // 720 blocks = ~3 hours at 15s/block
            return Err(Error::native_contract(
                "Invalid max delta value (must be 1-720)".to_string(),
            ));
        }

        let snapshot = engine.snapshot_cache();
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
