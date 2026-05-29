//! Notary native contract implementation.
//!
//! Mirrors the behaviour of `Neo.SmartContract.Native.Notary` exactly.
//! This contract assists with multisignature transaction forming by managing
//! GAS deposits for notary service fees.

use neo_crypto::Crypto;
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::network::p2p::payloads::{Transaction, TransactionAttributeType};
use crate::persistence::read_only_store::ReadOnlyStoreGeneric;
use crate::persistence::DataCache;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::native::helpers::NativeHelpers;
use crate::smart_contract::native::{
    gas_token::GasToken, ledger_contract::LedgerContract, policy_contract::PolicyContract,
    role_management::RoleManagement, NativeContract, NativeMethod, Role,
};
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::StorageItem;
use crate::neo_vm::{StackItem};
use crate::UInt160;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::sync::Arc;

mod deposit;
mod metadata;
mod native_impl;
pub use deposit::Deposit;
use deposit::{deserialize_deposit, serialize_deposit};

/// Storage key prefixes matching C# Notary contract.
const PREFIX_DEPOSIT: u8 = 1;
const PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;

/// Default maximum NotValidBefore delta (20 rounds for 7 validators).
const DEFAULT_MAX_NOT_VALID_BEFORE_DELTA: u32 = 140;
/// Default expiration delta applied when deposit owner cannot override the lock height.
const DEFAULT_DEPOSIT_DELTA_TILL: u32 = 5760;
/// Minimum number of blocks ahead of the current height a deposit must remain locked.
const MIN_DEPOSIT_LEAD: u32 = 2;

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

impl Notary {
    /// Contract ID for Notary (-10 per Neo protocol).
    pub const ID: i32 = -10;

    /// Creates a new Notary contract instance.
    pub fn new() -> Self {
        // Notary contract hash: 0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b
        // This is the official Neo N3 Notary contract hash computed from:
        // Helper.GetContractHash(UInt160.Zero, 0, "Notary")
        let hash = UInt160::parse("0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b")
            .expect("Valid Notary contract hash");

        Self {
            id: Self::ID,
            hash,
            methods: Self::native_methods(),
        }
    }

    /// Gets storage key for a deposit.
    fn deposit_key(account: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(Self::ID, PREFIX_DEPOSIT, account)
    }

    /// Gets storage key for max not valid before delta.
    fn max_delta_key() -> StorageKey {
        StorageKey::create(Self::ID, PREFIX_MAX_NOT_VALID_BEFORE_DELTA)
    }

    fn parse_deposit_metadata(
        data: Option<&Vec<u8>>,
        default_owner: &UInt160,
    ) -> Result<(UInt160, u32)> {
        let bytes = data.ok_or_else(|| {
            Error::native_contract("`data` parameter should be an array of 2 elements")
        })?;

        let mut item =
            BinarySerializer::deserialize(bytes, &ExecutionEngineLimits::default(), None).map_err(
                |_| Error::native_contract("`data` parameter should be an array of 2 elements"),
            )?;

        if matches!(item, StackItem::ByteString(_) | StackItem::Buffer(_)) {
            let nested_bytes = item.as_bytes().map_err(|_| {
                Error::native_contract("Invalid deposit metadata: cannot convert to bytes".to_string())
            })?;
            item = BinarySerializer::deserialize(
                &nested_bytes,
                &ExecutionEngineLimits::default(),
                None,
            )
            .map_err(|_| {
                Error::native_contract(
                    "`data` parameter should be an array of 2 elements".to_string(),
                )
            })?;
        }

        let StackItem::Array(array) = item else {
            return Err(Error::native_contract(
                "`data` parameter should be an array of 2 elements".to_string(),
            ));
        };
        let array = array.items();

        if array.len() != 2 {
            return Err(Error::native_contract(
                "`data` parameter should be an array of 2 elements".to_string(),
            ));
        }

        let owner = if array[0].is_null() {
            *default_owner
        } else {
            let bytes = array[0]
                .as_bytes()
                .map_err(|_| Error::native_contract("Invalid deposit owner: cannot convert to bytes".to_string()))?;
            if bytes.len() != UInt160::LENGTH {
                return Err(Error::native_contract(
                    "Deposit owner must be 20 bytes".to_string(),
                ));
            }
            UInt160::from_bytes(&bytes)
                .map_err(|_| Error::native_contract("Invalid deposit recipient"))?
        };

        let till_value = array[1].as_int().map_err(|err| {
            Error::native_contract(format!("Invalid deposit expiration: {}", err))
        })?;
        let till = till_value
            .to_u32()
            .ok_or_else(|| Error::native_contract("Deposit expiration must fit in u32"))?;

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
                let data = item.value_bytes();
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
                let data = item.value_bytes();
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
        // C# parity: stored as variable-width signed-LE BigInteger bytes (StorageItem.Value),
        // not a fixed 4-byte u32. Empty bytes ⇒ 0; parse via BigInt::from_signed_bytes_le.
        let key = Self::max_delta_key();
        match snapshot.try_get(&key) {
            Some(item) => {
                let data = item.value_bytes();
                if data.is_empty() {
                    0
                } else {
                    BigInt::from_signed_bytes_le(&data)
                        .to_u32()
                        .unwrap_or(DEFAULT_MAX_NOT_VALID_BEFORE_DELTA)
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

        if args.len() < 3 {
            return Err(Error::native_contract(
                "onNEP17Payment requires from, amount, and data arguments".to_string(),
            ));
        }

        let from = UInt160::from_bytes(&args[0])
            .map_err(|_| Error::native_contract("Invalid from address"))?;
        let amount = BigInt::from_signed_bytes_le(&args[1]);
        if amount.is_negative() {
            return Err(Error::native_contract(
                "Deposit amount cannot be negative".to_string(),
            ));
        }

        let snapshot = engine.snapshot_cache();
        let ledger = LedgerContract::new();
        let current_height = ledger
            .current_index(snapshot.as_ref())
            .unwrap_or_else(|_| NativeHelpers::current_index());
        let (deposit_owner, mut till) = Self::parse_deposit_metadata(args.get(2), &from)?;

        let tx_sender = engine
            .script_container()
            .and_then(|container| container.as_any().downcast_ref::<Transaction>())
            .and_then(Transaction::sender)
            .ok_or_else(|| Error::native_contract("onNEP17Payment requires transaction context"))?;
        let allowed_change_till = tx_sender == deposit_owner;

        if till < current_height.saturating_add(MIN_DEPOSIT_LEAD) {
            return Err(Error::native_contract(format!(
                "`till` shouldn't be less than the chain's height {} + 1",
                current_height.saturating_add(MIN_DEPOSIT_LEAD)
            )));
        }

        let key = Self::deposit_key(&deposit_owner);
        let (mut deposit, has_existing_deposit) = match snapshot.as_ref().try_get(&key) {
            Some(item) => (deserialize_deposit(&item.value_bytes())?, true),
            None => (Deposit::new(BigInt::zero(), 0), false),
        };

        if has_existing_deposit && till < deposit.till {
            return Err(Error::native_contract(format!(
                "`till` shouldn't be less than the previous value {}",
                deposit.till
            )));
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
                    "first deposit can not be less than {}, got {}",
                    min_required, amount
                )));
            }

            if !allowed_change_till {
                till = current_height.saturating_add(DEFAULT_DEPOSIT_DELTA_TILL);
            }
        } else if !allowed_change_till {
            till = deposit.till;
        }

        deposit.amount += amount;
        deposit.till = till;

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
            .map_err(|_| Error::native_contract("Invalid account address"))?;
        let till_value = BigInt::from_signed_bytes_le(&args[1]);
        let till = till_value
            .to_u32()
            .ok_or_else(|| Error::native_contract("Invalid till argument"))?;

        // Verify witness for account
        if !engine.check_witness_hash(&account)? {
            return Ok(vec![0]);
        }

        let snapshot = engine.snapshot_cache();
        let key = Self::deposit_key(&account);

        let current_deposit = match snapshot.as_ref().try_get(&key) {
            Some(item) => deserialize_deposit(&item.value_bytes())?,
            None => {
                return Ok(vec![0]);
            }
        };

        // Can only extend, not reduce lock period
        if till <= current_deposit.till {
            return Ok(vec![0]);
        }

        let ledger = LedgerContract::new();
        let current_height = ledger
            .current_index(snapshot.as_ref())
            .unwrap_or_else(|_| NativeHelpers::current_index());
        if till < current_height.saturating_add(MIN_DEPOSIT_LEAD) {
            return Ok(vec![0]);
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
            .map_err(|_| Error::native_contract("Invalid from address"))?;
        let to = if args[1].is_empty() {
            from
        } else {
            UInt160::from_bytes(&args[1])
                .map_err(|_| Error::native_contract("Invalid to address"))?
        };

        // Verify witness for from account
        if !engine.check_witness_hash(&from)? {
            return Ok(vec![0]);
        }

        let snapshot = engine.snapshot_cache();
        let key = Self::deposit_key(&from);

        let deposit = match snapshot.as_ref().try_get(&key) {
            Some(item) => deserialize_deposit(&item.value_bytes())?,
            None => {
                return Ok(vec![0]);
            }
        };

        // Check if deposit has expired
        let ledger = LedgerContract::new();
        let current_height = ledger
            .current_index(snapshot.as_ref())
            .unwrap_or_else(|_| NativeHelpers::current_index());
        if deposit.till > current_height {
            return Ok(vec![0]);
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
        engine.refresh_context_tracking()?;

        let call_result =
            engine.call_native_contract(GasToken::new().hash(), "transfer", &transfer_args);

        {
            let mut state = state_arc.lock();
            state.native_calling_script_hash = prev_native_caller;
            state.call_flags = prev_flags;
        }
        engine.refresh_context_tracking()?;

        let transfer_result = call_result?;
        if transfer_result.first().copied() != Some(1) {
            return Err(Error::native_contract(format!(
                "Transfer to {} has failed",
                to
            )));
        }

        Ok(vec![1]) // true
    }

    /// Set maximum NotValidBefore delta (committee only)
    fn set_max_not_valid_before_delta(
        &self,
        engine: &mut ApplicationEngine,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "setMaxNotValidBeforeDelta requires value argument".to_string(),
            ));
        }

        let value = BigInt::from_signed_bytes_le(&args[0])
            .to_u32()
            .ok_or_else(|| {
                Error::native_contract(
                    "setMaxNotValidBeforeDelta requires value argument".to_string(),
                )
            })?;
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
        let min_allowed =
            u32::try_from(engine.protocol_settings().validators_count.max(0)).unwrap_or(0);
        let max_allowed = max_valid_increment.saturating_div(2);

        if value < min_allowed || value > max_allowed {
            return Err(Error::native_contract(format!(
                "MaxNotValidBeforeDelta cannot be more than {} or less than {}",
                max_allowed, min_allowed
            )));
        }

        // Verify committee witness against current committee address.
        let committee_address =
            NativeHelpers::committee_address(engine.protocol_settings(), Some(snapshot.as_ref()));
        if !engine.check_witness_hash(&committee_address)? {
            return Err(Error::native_contract(
                "setMaxNotValidBeforeDelta requires committee witness".to_string(),
            ));
        }

        let key = Self::max_delta_key();
        let value_bytes = BigInt::from(value).to_signed_bytes_le();
        if snapshot.as_ref().try_get(&key).is_some() {
            snapshot.update(key, StorageItem::from_bytes(value_bytes));
        } else {
            snapshot.add(key, StorageItem::from_bytes(value_bytes));
        }

        Ok(vec![])
    }

    fn verify(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> Result<Vec<u8>> {
        if args.is_empty() {
            return Err(Error::native_contract(
                "verify requires signature argument".to_string(),
            ));
        }

        let signature = &args[0];
        if signature.len() != 64 {
            return Ok(vec![0]);
        }

        let Some(tx) = engine
            .script_container()
            .and_then(|container| container.as_any().downcast_ref::<Transaction>())
        else {
            return Ok(vec![0]);
        };

        if tx
            .get_attribute(TransactionAttributeType::NotaryAssisted)
            .is_none()
        {
            return Ok(vec![0]);
        }

        for signer in tx.signers() {
            if signer.account == self.hash() {
                if signer.scopes != crate::WitnessScope::NONE {
                    return Ok(vec![0]);
                }
                break;
            }
        }

        if tx.sender() == Some(self.hash()) {
            if tx.signers().len() != 2 {
                return Ok(vec![0]);
            }
            let payer = tx.signers()[1].account;
            let snapshot = engine.snapshot_cache();
            let deposit = snapshot
                .as_ref()
                .try_get(&Self::deposit_key(&payer))
                .and_then(|item| deserialize_deposit(&item.value_bytes()).ok());
            let Some(deposit) = deposit else {
                return Ok(vec![0]);
            };
            let total_fee = tx.system_fee() + tx.network_fee();
            if deposit.amount < BigInt::from(total_fee) {
                return Ok(vec![0]);
            }
        }

        let snapshot = engine.snapshot_cache();
        let ledger = LedgerContract::new();
        let current_height = ledger
            .current_index(snapshot.as_ref())
            .unwrap_or_else(|_| NativeHelpers::current_index());
        let notaries = RoleManagement::new()
            .get_designated_by_role_at(snapshot.as_ref(), Role::P2PNotary, current_height + 1)
            .map_err(|err| {
                Error::native_contract(format!("Failed to read notary nodes: {}", err))
            })?;
        let sign_data =
            crate::network::p2p::helper::get_sign_data_vec(tx, engine.protocol_settings().network)
                .map_err(|err| {
                    Error::native_contract(format!("Failed to compute sign data: {}", err))
                })?;

        let valid = notaries
            .iter()
            .any(|notary| Crypto::verify_signature_bytes(&sign_data, signature, notary.as_bytes()));

        Ok(vec![if valid { 1 } else { 0 }])
    }

    fn get_notary_nodes(&self, snapshot: &DataCache) -> Result<Vec<neo_crypto::ECPoint>> {
        let ledger = LedgerContract::new();
        let current_height = ledger
            .current_index(snapshot)
            .unwrap_or_else(|_| NativeHelpers::current_index());
        RoleManagement::new().get_designated_by_role_at(
            snapshot,
            Role::P2PNotary,
            current_height + 1,
        )
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
}
