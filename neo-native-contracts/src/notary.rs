//! Notary native contract (id -10).
//!
//! Implements the method surface of the C# `Neo.SmartContract.Native.Notary`:
//! the `getMaxNotValidBeforeDelta` / `setMaxNotValidBeforeDelta` setting, the
//! GAS-deposit lifecycle (`onNEP17Payment`, `balanceOf`, `expirationOf`,
//! `lockDepositUntil`, `withdraw`), and `verify` — the notary-witness check
//! that validates a designated P2PNotary node's signature over the
//! transaction sign-data.

use std::any::Any;
use std::sync::LazyLock;

use neo_config::{Hardfork, ProtocolSettings};
use neo_error::{CoreError, CoreResult};
use neo_execution::application_engine_contract::NativeArgNullMask;
use neo_execution::{ApplicationEngine, Contract, NativeContract, NativeMethod};
use neo_payloads::{Transaction, TransactionAttribute, get_sign_data};
use neo_primitives::{
    CallFlags, ContractParameterType, TransactionAttributeType, UInt160, WitnessScope,
};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::Interoperable;
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::hashes::NOTARY_HASH;
use crate::{GasToken, LedgerContract, Role, RoleManagement};

/// C# `Notary.DefaultDepositDeltaTill`: the default lock-height delta applied to a
/// first deposit whose `till` the depositor isn't allowed to set itself.
const DEFAULT_DEPOSIT_DELTA_TILL: u32 = 5760;

/// Storage prefix for the max-NotValidBefore-delta setting (C#
/// `Notary.Prefix_MaxNotValidBeforeDelta`).
const PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;
/// C# `Notary.DefaultMaxNotValidBeforeDelta`.
const DEFAULT_MAX_NOT_VALID_BEFORE_DELTA: i64 = 140;
/// C# `Notary.Prefix_Deposit` — per-account deposit (`Struct[Amount, Till]`).
const PREFIX_DEPOSIT: u8 = 1;

/// C# `Notary.Deposit`: `Struct[Amount, Till]`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DepositState {
    amount: BigInt,
    till: u32,
}

impl DepositState {
    fn new(amount: BigInt, till: u32) -> Self {
        Self { amount, till }
    }

    fn to_stack_value(&self) -> StackValue {
        StackValue::Struct(
            0,
            vec![
                StackValue::BigInteger(self.amount.to_signed_bytes_le()),
                StackValue::Integer(i64::from(self.till)),
            ],
        )
    }

    fn from_stack_value(stack_value: StackValue) -> CoreResult<Self> {
        let StackValue::Struct(0, items) = stack_value else {
            return Err(CoreError::invalid_data("Notary deposit is not a struct"));
        };
        let amount_value = items
            .first()
            .ok_or_else(|| CoreError::invalid_data("Notary deposit Amount missing"))?;
        let amount = neo_vm_rs::stack_value_as_bigint(amount_value)
            .map_err(|e| CoreError::invalid_data(format!("Notary deposit Amount: {e}")))?;
        let till = items
            .get(1)
            .and_then(neo_vm_rs::stack_value_as_u32)
            .ok_or_else(|| CoreError::invalid_data("Notary deposit Till out of range"))?;
        Ok(Self { amount, till })
    }
}

neo_vm::impl_interoperable_via_stack_value!(DepositState);

/// The Notary native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct Notary;

impl Notary {
    /// Stable native contract id (matches C# `Notary`).
    pub const ID: i32 = -10;
    /// Stable native contract name (matches C# `Notary.Name`).
    pub const NAME: &'static str = "Notary";

    /// Construct a new `Notary` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the Notary script hash.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the Notary script hash.
    pub fn script_hash() -> UInt160 {
        *NOTARY_HASH
    }

    /// Reads field `index` of the C# `Deposit` struct (`[Amount, Till]`) stored under
    /// `Prefix_Deposit ++ account`, returning 0 when the account has no deposit.
    /// `balanceOf` reads `Amount` (index 0); `expirationOf` reads `Till` (index 1).
    fn read_deposit_field(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
        index: usize,
    ) -> CoreResult<BigInt> {
        let key = StorageKey::new(
            Notary::ID,
            crate::keys::prefixed_with_hash160(PREFIX_DEPOSIT, account),
        );
        let Some(item) = snapshot.get(&key) else {
            return Ok(BigInt::from(0));
        };
        let bytes = item.value_bytes();
        let (amount, till) = Self::decode_deposit(bytes.as_ref())?;
        match index {
            0 => Ok(amount),
            1 => Ok(BigInt::from(till)),
            _ => Err(CoreError::invalid_data("Notary deposit field is missing")),
        }
    }

    /// The deposit storage key `(Notary.ID, [Prefix_Deposit, account])`.
    fn deposit_key(account: &UInt160) -> StorageKey {
        StorageKey::new(
            Notary::ID,
            crate::keys::prefixed_with_hash160(PREFIX_DEPOSIT, account),
        )
    }

    fn decode_deposit(bytes: &[u8]) -> CoreResult<(BigInt, u32)> {
        let limits = ExecutionEngineLimits::default();
        let state = BinarySerializer::deserialize_stack_value_with_limits(
            bytes,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::deserialization(format!("Notary deposit: {e}")))?;
        let deposit = DepositState::from_stack_value(state)
            .map_err(|e| CoreError::invalid_data(format!("Notary deposit: {e}")))?;
        Ok((deposit.amount, deposit.till))
    }

    /// Reads the full `Deposit` `(Amount, Till)` for `account`, or `None` when the
    /// account has no deposit (C# `GetDepositFor` returning null).
    fn read_deposit(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
    ) -> CoreResult<Option<(BigInt, u32)>> {
        let Some(item) = snapshot.get(&Self::deposit_key(account)) else {
            return Ok(None);
        };
        let bytes = item.value_bytes();
        Self::decode_deposit(bytes.as_ref()).map(Some)
    }

    /// Deletes the deposit entry for `account` (C# `RemoveDepositFor`).
    fn delete_deposit(&self, snapshot: &DataCache, account: &UInt160) {
        snapshot.delete(&Self::deposit_key(account));
    }

    /// Writes the `Deposit` `(Amount, Till)` struct for `account` (C# `PutDepositFor`):
    /// the BinarySerialized `Struct[Amount, Till]`.
    fn write_deposit(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
        amount: &BigInt,
        till: u32,
    ) -> CoreResult<()> {
        let item = DepositState::new(amount.clone(), till).to_stack_value();
        let bytes = BinarySerializer::serialize_stack_value_default(&item)
            .map_err(|e| CoreError::serialization(format!("Notary deposit serialize: {e}")))?;
        snapshot.update(Self::deposit_key(account), StorageItem::from_bytes(bytes));
        Ok(())
    }

    /// Pure decision core of C# `LockDepositUntil` (after the witness check): returns
    /// `Some((amount, till))` to write, or `None` to return `false`. The new `till`
    /// must be at least `current_index + 2` (so the deposit outlives the next block)
    /// and at least the deposit's existing `Till` (locks cannot be shortened), and a
    /// deposit must already exist. `wrapping_add` matches C#'s unchecked `uint` math.
    fn lock_deposit_decision(
        current_index: u32,
        deposit: Option<(BigInt, u32)>,
        till: u32,
    ) -> Option<(BigInt, u32)> {
        if till < current_index.wrapping_add(2) {
            return None;
        }
        let (amount, existing_till) = deposit?;
        if till < existing_till {
            return None;
        }
        Some((amount, till))
    }

    /// Parses the `onNEP17Payment` `data` argument (an `Any` param that arrives
    /// BinarySerialized). C# requires it to be an `Array` of exactly 2 elements:
    /// `[to, till]` where `to` is `Null` (use the GAS sender `from`) or a `UInt160`,
    /// and `till` is the requested lock height.
    fn parse_onnep17_data(from: &UInt160, data: &[u8]) -> CoreResult<(UInt160, u32)> {
        let limits = ExecutionEngineLimits::default();
        let item = BinarySerializer::deserialize_stack_value_with_limits(
            data,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
        .map_err(|e| CoreError::invalid_operation(format!("Notary::onNEP17Payment data: {e}")))?;
        let StackValue::Array(0, items) = item else {
            return Err(CoreError::invalid_operation(
                "Notary::onNEP17Payment data must be an array of 2 elements",
            ));
        };
        if items.len() != 2 {
            return Err(CoreError::invalid_operation(
                "Notary::onNEP17Payment data must be an array of 2 elements",
            ));
        }
        let to = if matches!(items[0], StackValue::Null) {
            *from
        } else {
            let bytes = items[0].to_byte_string_bytes().ok_or_else(|| {
                CoreError::invalid_operation("Notary::onNEP17Payment to: cannot convert to bytes")
            })?;
            crate::args::bytes_to_hash160(&bytes, "Notary::onNEP17Payment to: bad hash")?
        };
        let till_value = items[1].to_i128().ok_or_else(|| {
            CoreError::invalid_operation("Notary::onNEP17Payment till: cannot convert to integer")
        })?;
        let till = u32::try_from(till_value).map_err(|_| {
            CoreError::invalid_operation("Notary::onNEP17Payment till out of uint range")
        })?;
        Ok((to, till))
    }

    /// The pure deposit decision of C# `Notary.OnNEP17Payment` (after the GAS-caller
    /// and data checks). Returns the `(Amount, Till)` to write, or an error string
    /// describing the C# fault. `existing` is the current deposit for `to`
    /// (`None` = first deposit). `wrapping_add` matches C#'s unchecked `uint` math.
    fn compute_deposit(
        existing: Option<(BigInt, u32)>,
        amount: &BigInt,
        till: u32,
        allowed_change_till: bool,
        current_height: u32,
        fee_per_key: i64,
    ) -> Result<(BigInt, u32), &'static str> {
        if till < current_height.wrapping_add(2) {
            return Err("`till` is below the chain height + 2");
        }
        match existing {
            Some((existing_amount, existing_till)) => {
                if till < existing_till {
                    return Err("`till` is below the previous deposit Till");
                }
                // An existing deposit only adopts the requested `till` when the GAS
                // sender is the deposit owner; otherwise the lock height is unchanged.
                let final_till = if allowed_change_till {
                    till
                } else {
                    existing_till
                };
                Ok((existing_amount + amount, final_till))
            }
            None => {
                // First deposit must be at least 2 * the NotaryAssisted attribute fee.
                let minimum = BigInt::from(2) * BigInt::from(fee_per_key);
                if amount < &minimum {
                    return Err("first deposit is below 2 * the NotaryAssisted fee");
                }
                let final_till = if allowed_change_till {
                    till
                } else {
                    current_height.wrapping_add(DEFAULT_DEPOSIT_DELTA_TILL)
                };
                Ok((amount.clone(), final_till))
            }
        }
    }

    /// C# `SetMaxNotValidBeforeDelta` storage effect: overwrite
    /// `Prefix_MaxNotValidBeforeDelta` (`GetAndChange(...).Set(value)`). The key is
    /// genesis-initialised (`OnPersist` Add), so `update` (= C# GetAndChange) is the
    /// correct primitive.
    fn put_max_not_valid_before_delta(&self, snapshot: &DataCache, value: i64) {
        snapshot.update(
            StorageKey::new(Notary::ID, vec![PREFIX_MAX_NOT_VALID_BEFORE_DELTA]),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(value))),
        );
    }

    /// C# `GetMaxNotValidBeforeDelta` directly indexes the initialized setting
    /// storage item; a missing key faults instead of silently using the default.
    fn read_max_not_valid_before_delta(&self, snapshot: &DataCache) -> CoreResult<i64> {
        let key = StorageKey::new(Notary::ID, vec![PREFIX_MAX_NOT_VALID_BEFORE_DELTA]);
        let Some(item) = snapshot.get(&key) else {
            return Err(CoreError::invalid_data(
                "Notary MaxNotValidBeforeDelta is missing",
            ));
        };
        BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_i64()
            .ok_or_else(|| {
                CoreError::invalid_operation("Notary MaxNotValidBeforeDelta out of range")
            })
    }

    /// Parses the leading `Hash160` account argument for the deposit reads.
    fn parse_account(args: &[Vec<u8>], method: &str) -> CoreResult<UInt160> {
        crate::args::raw_account(args, &format!("Notary::{method}"))
    }
}

static NOTARY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    let int = ContractParameterType::Integer;
    vec![
        NativeMethod::new(
            "getMaxNotValidBeforeDelta".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            int,
        ),
        // Deposit reads: balanceOf -> Amount, expirationOf -> Till.
        NativeMethod::new(
            "balanceOf".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            int,
        )
        .with_parameter_names(["account"]),
        NativeMethod::new(
            "expirationOf".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            int,
        )
        .with_parameter_names(["account"]),
        // Committee-gated setter: not safe, States, Integer -> Void.
        NativeMethod::new(
            "setMaxNotValidBeforeDelta".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![int],
            ContractParameterType::Void,
        )
        .with_parameter_names(["value"]),
        // lockDepositUntil(account, till) -> bool: account-witnessed, States.
        NativeMethod::new(
            "lockDepositUntil".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Hash160, int],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["account", "till"]),
        // onNEP17Payment(from, amount, data) -> Void: GAS deposit callback, States.
        NativeMethod::new(
            "onNEP17Payment".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![
                ContractParameterType::Hash160,
                int,
                ContractParameterType::Any,
            ],
            ContractParameterType::Void,
        )
        .with_parameter_names(["from", "amount", "data"]),
        // withdraw(from, to?) -> bool: depositor-witnessed; transfers the unlocked
        // deposit GAS from Notary to `to` (re-entrant, CallFlags.All).
        NativeMethod::new(
            "withdraw".to_string(),
            1 << 15,
            false,
            CallFlags::ALL.bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
            ],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["from", "to"]),
        // verify(signature) -> bool: notary-witness verification. C#
        // `[ContractMethod(CpuFee = 1 << 15, RequiredCallFlags = CallFlags.ReadStates)]`
        // (Notary.cs Verify), and ContractMethodMetadata derives
        // `Safe = (ReadStates & ~CallFlags.ReadOnly) == 0` -> manifest-safe.
        NativeMethod::new(
            "verify".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::ByteArray],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["signature"]),
    ]
});

impl NativeContract for Notary {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    // C# `Notary.Activations => [Hardfork.HF_Echidna, Hardfork.HF_Faun]`
    // (Notary.cs): the contract itself does not exist before HF_Echidna —
    // `ActiveIn` is the first activation. Without this override the contract
    // would be genesis-active in neo-rs, diverging native deployment, manifest
    // state, and call resolution below the Echidna height.
    fn active_in(&self) -> Option<Hardfork> {
        Some(Hardfork::HfEchidna)
    }

    fn activations(&self) -> Vec<Hardfork> {
        vec![Hardfork::HfEchidna, Hardfork::HfFaun]
    }

    /// C# `Notary.OnManifestCompose` (Notary.cs:92-102): NEP-30 joins NEP-27
    /// once HF_Faun is enabled at the height.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            vec!["NEP-27".to_string(), "NEP-30".to_string()]
        } else {
            vec!["NEP-27".to_string()]
        }
    }

    fn methods(&self) -> &[NativeMethod] {
        &NOTARY_METHODS
    }

    /// C# `Notary.InitializeAsync(engine, hardfork)` for `hardfork == ActiveIn`
    /// (Notary.cs:52-59; ActiveIn is HF_Echidna, so this runs while persisting
    /// the Echidna activation block): seed `Prefix_MaxNotValidBeforeDelta` with
    /// `DefaultMaxNotValidBeforeDelta` (140).
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        engine.snapshot_cache().add(
            StorageKey::new(Self::ID, vec![PREFIX_MAX_NOT_VALID_BEFORE_DELTA]),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MAX_NOT_VALID_BEFORE_DELTA,
            ))),
        );
        Ok(())
    }

    /// C# `Notary.OnPersistAsync` (Notary.cs:61-90), run by the persist
    /// pipeline only while Notary is active (`ActiveIn = HF_Echidna`, gated
    /// by `is_active` in the dispatch loop).
    ///
    /// For every transaction in the persisting block carrying a
    /// `NotaryAssisted` attribute it (a) accumulates `nKeys + 1` into the
    /// fee count and (b) — when the transaction's sender is the Notary
    /// account itself — debits the payer's (`Signers[1]`) deposit by
    /// `SystemFee + NetworkFee`, removing the deposit at zero. After the
    /// loop it mints the per-notary reward `nFees *
    /// Policy.GetAttributeFeeV1(NotaryAssisted) / notaries.Length` (C#
    /// `CalculateNotaryReward`) to each designated P2PNotary node's
    /// signature-redeem-script hash. This is the reminting counterpart of
    /// the NotaryAssisted share `GasToken::on_persist` withholds from the
    /// primary-validator network-fee mint, so per-block GAS supply is
    /// conserved (matching C#, including the dropped integer-division
    /// remainder).
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let notary_hash = Self::script_hash();

        // Pass 1: under the persisting-block borrow, accumulate the fee
        // count and the Notary-paid deposit debits.
        let (n_fees, debits) = {
            let block = engine.persisting_block().ok_or_else(|| {
                CoreError::invalid_operation("Notary::on_persist requires a persisting block")
            })?;
            let mut n_fees: i64 = 0;
            let mut debits: Vec<(UInt160, i64)> = Vec::new();
            for tx in &block.transactions {
                // C# `tx.GetAttribute<NotaryAssisted>()` (AllowMultiple=false).
                let Some(nkeys) = tx.attributes().iter().find_map(|attr| match attr {
                    TransactionAttribute::NotaryAssisted(na) => Some(na.nkeys),
                    _ => None,
                }) else {
                    continue;
                };
                n_fees += i64::from(nkeys) + 1;
                // C# `if (tx.Sender == Hash)`: the Notary pays the fees, so
                // debit the payer (`Signers[1]`) deposit.
                if tx.sender() == Some(notary_hash) {
                    let payer = tx.signers().get(1).ok_or_else(|| {
                        CoreError::invalid_operation(
                            "Notary::on_persist: notary-paid transaction has fewer than two signers",
                        )
                    })?;
                    // C# `tx.SystemFee + tx.NetworkFee` (unchecked long).
                    let fees = tx.system_fee().wrapping_add(tx.network_fee());
                    debits.push((payer.account, fees));
                }
            }
            (n_fees, debits)
        };

        // C# `if (nFees == 0) return;` — no NotaryAssisted transactions.
        if n_fees == 0 {
            return Ok(());
        }

        // Apply the deposit debits staged above (C# `GetAndChange(
        // Prefix_Deposit, payer)` inside the transaction loop): subtract the
        // fees, removing the deposit when it reaches zero.
        {
            let snapshot = engine.snapshot_cache();
            for (payer, fees) in &debits {
                if let Some((amount, till)) = self.read_deposit(&snapshot, payer)? {
                    let new_amount = amount - BigInt::from(*fees);
                    if new_amount.sign() == num_bigint::Sign::NoSign {
                        self.delete_deposit(&snapshot, payer);
                    } else {
                        self.write_deposit(&snapshot, payer, &new_amount, till)?;
                    }
                }
            }
        }

        // C# `GetNotaryNodes`: the P2PNotary designation effective at
        // `Ledger.CurrentIndex + 1`.
        let notaries = {
            let snapshot = engine.snapshot_cache();
            let current = LedgerContract::new().current_index(&snapshot)?;
            RoleManagement::new().get_designated_by_role_at(
                &snapshot,
                Role::P2PNotary,
                current.wrapping_add(1),
            )?
        };
        // C# divides the reward by `notaries.Length`; an empty designation
        // with NotaryAssisted fees would be a DivideByZeroException faulting
        // the block (unreachable for a valid block — NotaryAssisted
        // verification requires designated notaries).
        if notaries.is_empty() {
            return Err(CoreError::invalid_operation(
                "Notary::on_persist: NotaryAssisted fees with no designated P2PNotary nodes",
            ));
        }

        // C# `CalculateNotaryReward`: `nFees * GetAttributeFeeV1(
        // NotaryAssisted) / notaries.Length`, minted to each notary with
        // `callOnPayment = false`.
        let per_key = crate::PolicyContract::new().attribute_fee(
            &engine.snapshot_cache(),
            TransactionAttributeType::NotaryAssisted.to_byte(),
            true,
        )?;
        let single_reward = BigInt::from(n_fees.wrapping_mul(per_key) / notaries.len() as i64);
        for notary in notaries {
            let address = UInt160::from_script(&Contract::create_signature_redeem_script(notary));
            crate::GasToken::new().gas_mint(engine, &address, &single_reward, false)?;
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "getMaxNotValidBeforeDelta" => {
                let delta = self.read_max_not_valid_before_delta(&snapshot)?;
                Ok(BigInt::from(delta).to_signed_bytes_le())
            }
            "balanceOf" => {
                let account = Self::parse_account(args, "balanceOf")?;
                Ok(self
                    .read_deposit_field(&snapshot, &account, 0)?
                    .to_signed_bytes_le())
            }
            "expirationOf" => {
                let account = Self::parse_account(args, "expirationOf")?;
                Ok(self
                    .read_deposit_field(&snapshot, &account, 1)?
                    .to_signed_bytes_le())
            }
            "lockDepositUntil" => {
                // C#: CheckWitnessInternal(account) (false return on no witness),
                // then till >= currentIndex+2, an existing deposit, and till not
                // shortening it; on success update Deposit.Till and write back.
                let account = Self::parse_account(args, "lockDepositUntil")?;
                let till = args
                    .get(1)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "Notary::lockDepositUntil requires a uint till",
                        )
                    })?;
                // CheckWitnessInternal: a missing witness returns false (not a fault).
                let witnessed = engine.check_witness(&account).map_err(|e| {
                    CoreError::invalid_operation(format!("lockDepositUntil witness: {e}"))
                })?;
                if !witnessed {
                    return Ok(vec![0]);
                }
                let current = LedgerContract::new().current_index(&snapshot)?;
                let deposit = self.read_deposit(&snapshot, &account)?;
                match Self::lock_deposit_decision(current, deposit, till) {
                    Some((amount, new_till)) => {
                        self.write_deposit(&engine.snapshot_cache(), &account, &amount, new_till)?;
                        Ok(vec![1])
                    }
                    None => Ok(vec![0]),
                }
            }
            "onNEP17Payment" => {
                // C#: only GAS may deposit; data = Array[to?, till]; the deposit
                // owner (tx.Sender == to) may set the lock height.
                let from = Self::parse_account(args, "onNEP17Payment")?;
                let amount = args
                    .get(1)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .ok_or_else(|| {
                        CoreError::invalid_operation("Notary::onNEP17Payment requires an amount")
                    })?;
                let data = args.get(2).map(Vec::as_slice).unwrap_or(&[]);

                if engine.get_calling_script_hash() != Some(GasToken::script_hash()) {
                    return Err(CoreError::invalid_operation(
                        "Notary::onNEP17Payment: only GAS can be accepted for deposit",
                    ));
                }
                let (to, till) = Self::parse_onnep17_data(&from, data)?;
                // C# `allowedChangeTill = tx.Sender == to`; the script container is
                // the persisting transaction (the GAS transfer that triggered this).
                let sender = engine
                    .script_container()
                    .and_then(|c| c.as_any().downcast_ref::<Transaction>())
                    .and_then(|tx| tx.sender());
                let allowed_change_till = sender == Some(to);

                let current = LedgerContract::new().current_index(&snapshot)?;
                let fee_per_key = crate::PolicyContract::new().attribute_fee(
                    &snapshot,
                    TransactionAttributeType::NotaryAssisted.to_byte(),
                    true,
                )?;
                let existing = self.read_deposit(&snapshot, &to)?;
                match Self::compute_deposit(
                    existing,
                    &amount,
                    till,
                    allowed_change_till,
                    current,
                    fee_per_key,
                ) {
                    Ok((new_amount, new_till)) => {
                        self.write_deposit(&engine.snapshot_cache(), &to, &new_amount, new_till)?;
                        Ok(Vec::new())
                    }
                    Err(msg) => Err(CoreError::invalid_operation(format!(
                        "Notary::onNEP17Payment: {msg}"
                    ))),
                }
            }
            "withdraw" => {
                // C# Withdraw(from, to?): witness the depositor, then transfer the
                // unlocked deposit GAS from Notary to `to` (defaulting to `from`).
                let from = Self::parse_account(args, "withdraw")?;
                // `to` is a nullable UInt160?: a Null arg (bit 1 of the native arg
                // null-mask) means "send to `from`".
                let to_is_null = engine
                    .get_state::<NativeArgNullMask>()
                    .is_some_and(|mask| mask.0 & (1 << 1) != 0);
                let receive = if to_is_null {
                    from
                } else {
                    crate::args::raw_hash160(args, 1, "Notary::withdraw")?
                };

                let witnessed = engine
                    .check_witness(&from)
                    .map_err(|e| CoreError::invalid_operation(format!("withdraw witness: {e}")))?;
                if !witnessed {
                    return Ok(vec![0]);
                }
                let Some((amount, till)) = self.read_deposit(&snapshot, &from)? else {
                    return Ok(vec![0]); // no deposit
                };
                if LedgerContract::new().current_index(&snapshot)? < till {
                    return Ok(vec![0]); // still locked
                }
                // C# removes the deposit BEFORE the transfer; a failed transfer
                // throws, which rolls back this delete with the rest of the call.
                self.delete_deposit(&engine.snapshot_cache(), &from);
                let notary_hash = Notary::script_hash();
                // from == caller == Notary, so the transfer's witness check passes
                // (Notary moves its own balance), faithful to the C# nested call.
                let ok = GasToken::transfer_core(
                    engine,
                    notary_hash,
                    &notary_hash,
                    &receive,
                    &amount,
                    &[],
                )?;
                if !ok {
                    return Err(CoreError::invalid_operation(format!(
                        "Notary::withdraw: transfer to {receive} failed"
                    )));
                }
                Ok(vec![1])
            }
            "verify" => {
                // C# Verify(engine, byte[] signature): the script container must
                // be a Transaction carrying a NotaryAssisted attribute whose
                // Notary-account signer (when present) has WitnessScope.None; a
                // Notary-paid transaction (Sender == Hash) must have exactly
                // [Notary, payer] signers with the payer's deposit covering
                // SystemFee + NetworkFee; finally `signature` must be a valid
                // secp256r1 signature over the tx sign-data (network magic ++
                // tx hash) by ONE of the designated P2PNotary nodes. Every
                // rejection returns false, never a fault.
                let signature_is_null = engine
                    .get_state::<NativeArgNullMask>()
                    .is_some_and(|mask| mask.0 & 1 != 0);
                let signature = args.first().map(Vec::as_slice).unwrap_or(&[]);
                if signature_is_null || signature.len() != 64 {
                    return Ok(vec![0]);
                }
                let Some(tx) = engine
                    .script_container()
                    .and_then(|c| c.as_any().downcast_ref::<Transaction>())
                else {
                    return Ok(vec![0]); // C# `engine.ScriptContainer as Transaction` null
                };
                if tx
                    .get_attribute(TransactionAttributeType::NotaryAssisted)
                    .is_none()
                {
                    return Ok(vec![0]);
                }
                let notary_hash = Self::script_hash();
                // The Notary-account signer must not request any witness scope.
                for signer in tx.signers() {
                    if signer.account == notary_hash {
                        if signer.scopes != WitnessScope::NONE {
                            return Ok(vec![0]);
                        }
                        break;
                    }
                }
                // C# `tx.Sender` is `Signers[0].Account`: a signer-less
                // transaction faults there rather than returning false.
                let sender = tx.sender().ok_or_else(|| {
                    CoreError::invalid_operation("Notary::verify: transaction has no signers")
                })?;
                if sender == notary_hash {
                    // Notary pays the fees: exactly [Notary, payer] signers and
                    // a deposit for the payer that covers the transaction fees.
                    if tx.signers().len() != 2 {
                        return Ok(vec![0]);
                    }
                    let payer = tx.signers()[1].account;
                    let Some((amount, _till)) = self.read_deposit(&snapshot, &payer)? else {
                        return Ok(vec![0]);
                    };
                    // C# `tx.NetworkFee + tx.SystemFee` is unchecked long math.
                    let fees = BigInt::from(tx.network_fee().wrapping_add(tx.system_fee()));
                    if amount < fees {
                        return Ok(vec![0]);
                    }
                }
                // C# GetNotaryNodes: the P2PNotary designation effective at
                // Ledger.CurrentIndex + 1.
                let current = LedgerContract::new().current_index(&snapshot)?;
                let notaries = RoleManagement::new().get_designated_by_role_at(
                    &snapshot,
                    Role::P2PNotary,
                    current.wrapping_add(1),
                )?;
                let network = engine.protocol_settings().network;
                let sign_data = get_sign_data(tx, network)?;
                // C# Crypto.VerifySignature returns false (never throws) for a
                // malformed 64-byte signature; map decode errors to false.
                let valid = notaries
                    .iter()
                    .any(|n| n.verify_signature(&sign_data, signature).unwrap_or(false));
                Ok(vec![u8::from(valid)])
            }
            "setMaxNotValidBeforeDelta" => {
                // C# param is `uint value`: decode as u32 (out-of-range faults like
                // the C# uint parameter binding).
                let value = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_u32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "Notary::setMaxNotValidBeforeDelta requires a uint value",
                        )
                    })?;
                // C# v3.10.0 bound: value must be ≤ GetMaxValidUntilBlockIncrement/2
                // and ≥ engine.ProtocolSettings.ValidatorsCount (was the constant
                // ProtocolSettings.Default.ValidatorsCount = 0). On a network whose
                // ValidatorsCount > 0 this now rejects small deltas the old check
                // let through — a tx-validity divergence.
                let upper = crate::PolicyContract::new()
                    .system_max_valid_until_block_increment(engine)?
                    / 2;
                let lower = i64::from(engine.protocol_settings().validators_count);
                if i64::from(value) > upper || i64::from(value) < lower {
                    return Err(CoreError::invalid_operation(format!(
                        "MaxNotValidBeforeDelta cannot be more than {upper} or less than {lower}"
                    )));
                }
                crate::committee::assert_committee(engine, "setMaxNotValidBeforeDelta")?;
                self.put_max_not_valid_before_delta(&engine.snapshot_cache(), i64::from(value));
                Ok(Vec::new())
            }
            other => Err(CoreError::invalid_operation(format!(
                "Notary method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::persistence::DataCache;
    use neo_storage::{StorageItem, StorageKey};
    use neo_vm::StackItem;

    #[test]
    fn native_contract_surface() {
        let c = Notary::new();
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "getMaxNotValidBeforeDelta",
                "balanceOf",
                "expirationOf",
                "setMaxNotValidBeforeDelta",
                "lockDepositUntil",
                "onNEP17Payment",
                "withdraw",
                "verify"
            ]
        );
        // verify: ReadStates, (ByteArray) -> Boolean. Manifest-SAFE: C#
        // derives Safe = (ReadStates & ~CallFlags.ReadOnly) == 0
        // (ContractMethodMetadata.cs:74).
        let verify = c.methods().iter().find(|m| m.name == "verify").unwrap();
        assert!(verify.safe);
        assert_eq!(verify.required_call_flags, CallFlags::READ_STATES.bits());
        assert_eq!(verify.parameters, vec![ContractParameterType::ByteArray]);
        assert_eq!(verify.return_type, ContractParameterType::Boolean);
        assert_eq!(verify.cpu_fee, 1 << 15);
        // withdraw: not safe, CallFlags.All (re-entrant GAS transfer),
        // (Hash160, Hash160) -> Boolean.
        let withdraw = c.methods().iter().find(|m| m.name == "withdraw").unwrap();
        assert!(!withdraw.safe);
        assert_eq!(withdraw.required_call_flags, CallFlags::ALL.bits());
        assert_eq!(
            withdraw.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160
            ]
        );
        assert_eq!(withdraw.return_type, ContractParameterType::Boolean);
        // onNEP17Payment: not safe, States, (Hash160, Integer, Any) -> Void.
        let on_pay = c
            .methods()
            .iter()
            .find(|m| m.name == "onNEP17Payment")
            .unwrap();
        assert!(!on_pay.safe);
        assert_eq!(on_pay.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(
            on_pay.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Integer,
                ContractParameterType::Any
            ]
        );
        assert_eq!(on_pay.return_type, ContractParameterType::Void);
        for name in ["balanceOf", "expirationOf"] {
            let m = c.methods().iter().find(|m| m.name == name).unwrap();
            assert_eq!(m.parameters, vec![ContractParameterType::Hash160]);
            assert_eq!(m.return_type, ContractParameterType::Integer);
        }
        // The committee-gated setter: not safe, States, Integer -> Void.
        let setter = c
            .methods()
            .iter()
            .find(|m| m.name == "setMaxNotValidBeforeDelta")
            .unwrap();
        assert!(!setter.safe);
        assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(setter.return_type, ContractParameterType::Void);
        assert_eq!(setter.cpu_fee, 1 << 15);
        // lockDepositUntil: not safe, States, (Hash160, Integer) -> Boolean.
        let lock = c
            .methods()
            .iter()
            .find(|m| m.name == "lockDepositUntil")
            .unwrap();
        assert!(!lock.safe);
        assert_eq!(lock.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(
            lock.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Integer
            ]
        );
        assert_eq!(lock.return_type, ContractParameterType::Boolean);
    }

    #[test]
    fn deposit_round_trips_and_lock_decision_matches_csharp() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[7u8; 20]).unwrap();

        // No deposit -> read_deposit None; lock decision -> None (false).
        assert!(
            Notary::new()
                .read_deposit(&cache, &account)
                .unwrap()
                .is_none()
        );
        assert!(Notary::lock_deposit_decision(100, None, 200).is_none());

        // Write a deposit (Amount=1000, Till=150) and read it back.
        Notary::new()
            .write_deposit(&cache, &account, &BigInt::from(1000), 150)
            .unwrap();
        let expected = BinarySerializer::serialize(
            &StackItem::from_struct(vec![StackItem::from_int(1000), StackItem::from_int(150)]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        assert_eq!(
            cache
                .get(&Notary::deposit_key(&account))
                .unwrap()
                .value_bytes()
                .as_ref(),
            expected.as_slice()
        );
        assert_eq!(
            Notary::new().read_deposit(&cache, &account).unwrap(),
            Some((BigInt::from(1000), 150))
        );

        let deposit = Notary::new().read_deposit(&cache, &account).unwrap();
        // till below current+2 -> None.
        assert!(Notary::lock_deposit_decision(199, deposit.clone(), 200).is_none());
        // till below existing Till (150) -> None (can't shorten).
        assert!(Notary::lock_deposit_decision(100, deposit.clone(), 149).is_none());
        // Valid extension keeps Amount, updates Till.
        assert_eq!(
            Notary::lock_deposit_decision(100, deposit, 300),
            Some((BigInt::from(1000), 300))
        );

        // The lock write preserves Amount and updates Till.
        Notary::new()
            .write_deposit(&cache, &account, &BigInt::from(1000), 300)
            .unwrap();
        assert_eq!(
            Notary::new().read_deposit(&cache, &account).unwrap(),
            Some((BigInt::from(1000), 300))
        );

        // withdraw's RemoveDepositFor: delete clears the entry.
        Notary::new().delete_deposit(&cache, &account);
        assert!(
            Notary::new()
                .read_deposit(&cache, &account)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn deposit_state_interoperable_projection_matches_csharp_shape() {
        let state = DepositState::new(BigInt::from(1000), 42);
        let expected_value = StackValue::Struct(
            0,
            vec![
                StackValue::BigInteger(BigInt::from(1000).to_signed_bytes_le()),
                StackValue::Integer(42),
            ],
        );

        assert_eq!(state.to_stack_value(), expected_value);

        let trait_value = Interoperable::to_stack_value(&state).unwrap();
        assert_eq!(trait_value, expected_value);

        let mut parsed = DepositState::new(BigInt::from(0), 0);
        Interoperable::from_stack_value(&mut parsed, trait_value).unwrap();
        assert_eq!(parsed, state);

        assert!(DepositState::from_stack_value(StackValue::Array(0, vec![])).is_err());
        assert!(
            DepositState::from_stack_value(StackValue::Struct(
                0,
                vec![StackValue::BigInteger(
                    BigInt::from(1000).to_signed_bytes_le()
                )]
            ))
            .is_err()
        );
    }

    #[test]
    fn deposit_storage_uses_stack_value_projection() {
        fn slice_between<'a>(source: &'a str, start: &str, end: &str) -> &'a str {
            let start_index = source.find(start).expect("start marker exists");
            let end_index = source[start_index..]
                .find(end)
                .map(|offset| start_index + offset)
                .expect("end marker exists");
            &source[start_index..end_index]
        }

        let source = include_str!("notary.rs");
        let writer = slice_between(source, "fn write_deposit(", "fn lock_deposit_decision");
        assert!(writer.contains("DepositState::new"));
        assert!(writer.contains("to_stack_value"));
        assert!(writer.contains("serialize_stack_value_default"));
        assert!(!writer.contains("StackValue::Struct"));
        assert!(!writer.contains("StackItem::from_struct"));
        assert!(!writer.contains("BinarySerializer::serialize("));

        let reader = slice_between(source, "fn decode_deposit(", "fn delete_deposit");
        assert!(reader.contains("deserialize_stack_value_with_limits"));
        assert!(reader.contains("DepositState::from_stack_value"));
        assert!(!reader.contains("stack_value_as_bigint"));
        assert!(!reader.contains("stack_value_as_u32"));
        assert!(!reader.contains("BinarySerializer::deserialize("));
    }

    #[test]
    fn compute_deposit_matches_csharp_onnep17_rules() {
        let amount = BigInt::from(100);
        // current=10 -> till must be >= 12.
        assert!(Notary::compute_deposit(None, &amount, 11, true, 10, 0).is_err());

        // First deposit below 2*feePerKey (fee=60 -> min 120) -> error.
        assert!(Notary::compute_deposit(None, &amount, 100, true, 10, 60).is_err());
        // First deposit, owner sets till (allowed) -> Amount=amount, Till=requested.
        assert_eq!(
            Notary::compute_deposit(None, &amount, 100, true, 10, 10).unwrap(),
            (BigInt::from(100), 100)
        );
        // First deposit, NOT owner -> till forced to current + DefaultDepositDeltaTill.
        assert_eq!(
            Notary::compute_deposit(None, &amount, 100, false, 10, 10).unwrap(),
            (BigInt::from(100), 10 + DEFAULT_DEPOSIT_DELTA_TILL)
        );

        // Existing deposit: till below previous Till -> error.
        assert!(
            Notary::compute_deposit(Some((BigInt::from(50), 200)), &amount, 150, true, 10, 0)
                .is_err()
        );
        // Existing, owner extends -> Amount accumulates, Till = requested.
        assert_eq!(
            Notary::compute_deposit(Some((BigInt::from(50), 200)), &amount, 300, true, 10, 0)
                .unwrap(),
            (BigInt::from(150), 300)
        );
        // Existing, NOT owner -> Amount accumulates, Till unchanged.
        assert_eq!(
            Notary::compute_deposit(Some((BigInt::from(50), 200)), &amount, 300, false, 10, 0)
                .unwrap(),
            (BigInt::from(150), 200)
        );
    }

    #[test]
    fn parse_onnep17_data_handles_null_and_explicit_to() {
        let from = UInt160::from_bytes(&[1u8; 20]).unwrap();
        let explicit = UInt160::from_bytes(&[2u8; 20]).unwrap();

        // [Null, 500] -> to defaults to `from`.
        let null_to = StackItem::from_array(vec![StackItem::null(), StackItem::from_int(500)]);
        let bytes =
            BinarySerializer::serialize(&null_to, &ExecutionEngineLimits::default()).unwrap();
        assert_eq!(
            Notary::parse_onnep17_data(&from, &bytes).unwrap(),
            (from, 500)
        );

        // [explicit_to, 700] -> to is the provided hash.
        let with_to = StackItem::from_array(vec![
            StackItem::from_byte_string(explicit.to_bytes()),
            StackItem::from_int(700),
        ]);
        let bytes2 =
            BinarySerializer::serialize(&with_to, &ExecutionEngineLimits::default()).unwrap();
        assert_eq!(
            Notary::parse_onnep17_data(&from, &bytes2).unwrap(),
            (explicit, 700)
        );

        // Wrong shape (not a 2-element array) -> error.
        let bad = StackItem::from_array(vec![StackItem::from_int(1)]);
        let bad_bytes =
            BinarySerializer::serialize(&bad, &ExecutionEngineLimits::default()).unwrap();
        assert!(Notary::parse_onnep17_data(&from, &bad_bytes).is_err());

        // C# Notary.OnNEP17Payment (Notary.cs:146-152) only inspects the
        // incoming StackItem's array/null/bytes/integer shape. The Rust parser
        // should use the shared StackValue projection rather than materializing
        // neo_vm::StackItem for this non-VM-inspection path.
        let source = include_str!("notary.rs");
        let start = source
            .find("fn parse_onnep17_data")
            .expect("parser source exists");
        let end = source[start..]
            .find("fn compute_deposit")
            .map(|offset| start + offset)
            .expect("next helper marker exists");
        let parser = &source[start..end];
        assert!(parser.contains("deserialize_stack_value_with_limits"));
        assert!(parser.contains("StackValue::Array"));
        assert!(parser.contains("StackValue::Null"));
        assert!(!parser.contains("BinarySerializer::deserialize("));
        assert!(!parser.contains("StackItem::Array"));
    }

    #[test]
    fn set_max_not_valid_before_delta_write_round_trips() {
        // The setter's storage effect (overwrite Prefix_MaxNotValidBeforeDelta) is
        // observed by the getMaxNotValidBeforeDelta reader, matching C#
        // GetAndChange(...).Set(value).
        let cache = DataCache::new(false);
        Notary::new().put_max_not_valid_before_delta(&cache, 250);
        assert_eq!(
            crate::read_storage_int(
                &cache,
                Notary::ID,
                PREFIX_MAX_NOT_VALID_BEFORE_DELTA,
                DEFAULT_MAX_NOT_VALID_BEFORE_DELTA
            )
            .unwrap(),
            250
        );
    }

    #[test]
    fn deposit_reads_amount_and_till_or_zero() {
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[3u8; 20]).unwrap();

        // Absent deposit -> both reads are 0.
        assert_eq!(
            Notary::new()
                .read_deposit_field(&cache, &account, 0)
                .unwrap(),
            BigInt::from(0)
        );
        assert_eq!(
            Notary::new()
                .read_deposit_field(&cache, &account, 1)
                .unwrap(),
            BigInt::from(0)
        );

        // Store a Deposit struct [Amount=1000, Till=42] and read each field.
        let deposit =
            StackItem::from_struct(vec![StackItem::from_int(1000), StackItem::from_int(42)]);
        let bytes =
            BinarySerializer::serialize(&deposit, &ExecutionEngineLimits::default()).unwrap();
        let mut key_bytes = vec![PREFIX_DEPOSIT];
        key_bytes.extend_from_slice(&account.to_bytes());
        cache.add(
            StorageKey::new(Notary::ID, key_bytes),
            StorageItem::from_bytes(bytes),
        );

        assert_eq!(
            Notary::new()
                .read_deposit_field(&cache, &account, 0)
                .unwrap(),
            BigInt::from(1000)
        ); // Amount
        assert_eq!(
            Notary::new()
                .read_deposit_field(&cache, &account, 1)
                .unwrap(),
            BigInt::from(42)
        ); // Till
    }

    #[test]
    fn max_not_valid_before_delta_requires_initialized_storage() {
        let cache = DataCache::new(false);
        let mut engine = ApplicationEngine::new(
            neo_primitives::TriggerType::Application,
            None,
            std::sync::Arc::new(cache),
            None,
            ProtocolSettings::default(),
            0,
            None,
        )
        .expect("engine builds");

        let err = Notary::new()
            .invoke(&mut engine, "getMaxNotValidBeforeDelta", &[])
            .expect_err("missing Notary max delta storage should fault");
        assert!(err.to_string().contains("MaxNotValidBeforeDelta"), "{err}");
    }
}

/// End-to-end coverage of `verify` through the VM dispatch (the proven
/// witness-gated script-execution harness): the Notary native is seeded via a
/// ContractManagement record, a P2PNotary designation is written in the
/// RoleManagement storage layout, and `verify(signature)` is exercised through
/// `System.Contract.Call` against NotaryAssisted transaction containers.
#[cfg(test)]
mod verify_dispatch_tests {
    use super::*;
    use neo_config::ProtocolSettings;

    /// ProtocolSettings with HF_Echidna scheduled from genesis — the Notary
    /// contract is Echidna-activated (C# `Notary.Activations`), so e2e calls at
    /// height 0 need it enabled (mirrors C# `TestProtocolSettings`).
    fn echidna_settings() -> ProtocolSettings {
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 0);
        settings
    }
    use crate::test_support::deploy_native;
    use neo_crypto::Secp256r1Crypto;
    use neo_execution::ApplicationEngine;
    use neo_execution::contract_state::ContractState;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_io::{BinaryWriter, Serializable};
    use neo_payloads::{Block, Header, NotaryAssisted, Signer, TransactionAttribute, Witness};
    use neo_primitives::{TriggerType, UInt256, Verifiable};
    use neo_vm::StackItem;
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::{OpCode, VmState};
    use std::sync::Arc;

    /// Writes a P2PNotary designation effective from block `index`: the
    /// RoleManagement record `(role_byte, index_be)` -> BinarySerializer array
    /// of compressed EC-point byte strings.
    fn seed_notary_designation(cache: &DataCache, index: u32, pubkeys: &[Vec<u8>]) {
        let list = StackItem::from_array(
            pubkeys
                .iter()
                .map(|p| StackItem::from_byte_string(p.clone()))
                .collect::<Vec<_>>(),
        );
        let value = BinarySerializer::serialize(&list, &ExecutionEngineLimits::default()).unwrap();
        let mut key = vec![Role::P2PNotary.as_byte()];
        key.extend_from_slice(&index.to_be_bytes());
        cache.add(
            StorageKey::new(RoleManagement::ID, key),
            StorageItem::from_bytes(value),
        );
    }

    fn seed_current_block(cache: &DataCache, index: u32) {
        let value = LedgerContract::new()
            .serialize_hash_index_state(&UInt256::default(), index)
            .expect("current block pointer");
        cache.add(
            StorageKey::new(LedgerContract::ID, vec![12]),
            StorageItem::from_bytes(value),
        );
    }

    /// A snapshot with the Notary native deployed and (optionally) the given
    /// compressed public keys designated as P2PNotary nodes from genesis.
    fn seeded_snapshot(notary_pubkeys: &[Vec<u8>]) -> Arc<DataCache> {
        crate::install();
        let cache = DataCache::new(false);
        seed_current_block(&cache, 0);
        deploy_native(
            &cache,
            &build_native_contract_state(&Notary, &echidna_settings(), 0),
        );
        if !notary_pubkeys.is_empty() {
            seed_notary_designation(&cache, 0, notary_pubkeys);
        }
        Arc::new(cache)
    }

    /// Calls `verify(signature)` on the Notary via System.Contract.Call with
    /// `container` as the script container; `signature: None` pushes Null.
    /// Returns the final VM state and the Boolean result.
    fn call_verify(
        snapshot: Arc<DataCache>,
        container: Option<Arc<dyn Verifiable>>,
        signature: Option<&[u8]>,
    ) -> (VmState, bool) {
        let mut builder = ScriptBuilder::new();
        match signature {
            Some(bytes) => {
                builder.emit_push(bytes);
            }
            None => {
                builder.emit_opcode(OpCode::PUSHNULL);
            }
        }
        builder.emit_push_int(1);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push("verify".as_bytes());
        builder.emit_push(&Notary::script_hash().to_array());
        builder.emit_syscall("System.Contract.Call").expect("call");

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            container,
            snapshot,
            None,
            echidna_settings(),
            10_00000000,
            None,
        )
        .expect("engine builds");
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        let result = engine
            .result_stack()
            .peek(0)
            .ok()
            .and_then(|item| item.as_bool().ok())
            .unwrap_or(false);
        (state, result)
    }

    /// A transaction carrying a NotaryAssisted attribute with the given signers.
    fn notary_assisted_tx(signers: Vec<Signer>) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_valid_until_block(100);
        tx.set_script(vec![0x40]); // RET
        tx.set_signers(signers);
        tx.set_attributes(vec![TransactionAttribute::NotaryAssisted(
            NotaryAssisted::new(1),
        )]);
        tx.set_witnesses(vec![Witness::empty()]);
        tx
    }

    /// Signs the C# `tx.GetSignData(network)` payload with the secp256r1 key.
    fn sign_tx(tx: &Transaction, private_key: &[u8; 32]) -> Vec<u8> {
        let sign_data = get_sign_data(tx, echidna_settings().network).unwrap();
        Secp256r1Crypto::sign(&sign_data, private_key)
            .unwrap()
            .to_vec()
    }

    #[test]
    fn verify_accepts_designated_notary_signature() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let snapshot = seeded_snapshot(&[pubkey]);

        let payer = UInt160::from_bytes(&[0x05; 20]).unwrap();
        let tx = notary_assisted_tx(vec![Signer::new(payer, WitnessScope::NONE)]);
        let signature = sign_tx(&tx, &private_key);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        // A designated notary's signature over the tx sign-data verifies.
        let (state, ok) = call_verify(
            Arc::clone(&snapshot),
            Some(Arc::clone(&container)),
            Some(&signature),
        );
        assert_eq!(state, VmState::HALT, "verify must HALT");
        assert!(ok, "designated notary signature must verify");

        // Tampering with one byte invalidates it (still a clean false).
        let mut tampered = signature.clone();
        tampered[10] ^= 0xFF;
        let (state2, ok2) = call_verify(snapshot, Some(container), Some(&tampered));
        assert_eq!(state2, VmState::HALT);
        assert!(!ok2, "tampered signature must not verify");
    }

    #[test]
    fn verify_rejects_missing_container_attribute_or_malformed_signature() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let snapshot = seeded_snapshot(&[pubkey]);
        let payer = UInt160::from_bytes(&[0x05; 20]).unwrap();

        // No script container -> false.
        let (state, ok) = call_verify(Arc::clone(&snapshot), None, Some(&[0u8; 64]));
        assert_eq!(state, VmState::HALT);
        assert!(!ok, "verify without a transaction container must be false");

        // A transaction WITHOUT the NotaryAssisted attribute -> false even with
        // a valid notary signature over its sign-data.
        let mut plain = Transaction::new();
        plain.set_valid_until_block(100);
        plain.set_script(vec![0x40]);
        plain.set_signers(vec![Signer::new(payer, WitnessScope::NONE)]);
        plain.set_witnesses(vec![Witness::empty()]);
        let signature = sign_tx(&plain, &private_key);
        let container: Arc<dyn Verifiable> = Arc::new(plain);
        let (state2, ok2) = call_verify(Arc::clone(&snapshot), Some(container), Some(&signature));
        assert_eq!(state2, VmState::HALT);
        assert!(!ok2, "verify requires the NotaryAssisted attribute");

        // Wrong signature length and Null signature -> false.
        let tx = notary_assisted_tx(vec![Signer::new(payer, WitnessScope::NONE)]);
        let container2: Arc<dyn Verifiable> = Arc::new(tx);
        let (state3, ok3) = call_verify(
            Arc::clone(&snapshot),
            Some(Arc::clone(&container2)),
            Some(&[1u8; 10]),
        );
        assert_eq!(state3, VmState::HALT);
        assert!(!ok3, "a 10-byte signature must be rejected");
        let (state4, ok4) = call_verify(snapshot, Some(container2), None);
        assert_eq!(state4, VmState::HALT);
        assert!(!ok4, "a Null signature must be rejected");
    }

    #[test]
    fn verify_rejects_when_no_notary_nodes_designated() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let snapshot = seeded_snapshot(&[]); // no designation

        let payer = UInt160::from_bytes(&[0x05; 20]).unwrap();
        let tx = notary_assisted_tx(vec![Signer::new(payer, WitnessScope::NONE)]);
        let signature = sign_tx(&tx, &private_key);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        let (state, ok) = call_verify(snapshot, Some(container), Some(&signature));
        assert_eq!(state, VmState::HALT);
        assert!(!ok, "no designated notaries -> false");
    }

    #[test]
    fn verify_requires_scope_none_on_the_notary_signer() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let snapshot = seeded_snapshot(&[pubkey]);

        // The Notary-account signer (second, so Sender stays the payer) carries
        // a non-None scope -> false despite the valid signature.
        let payer = UInt160::from_bytes(&[0x05; 20]).unwrap();
        let tx = notary_assisted_tx(vec![
            Signer::new(payer, WitnessScope::NONE),
            Signer::new(Notary::script_hash(), WitnessScope::GLOBAL),
        ]);
        let signature = sign_tx(&tx, &private_key);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        let (state, ok) = call_verify(Arc::clone(&snapshot), Some(container), Some(&signature));
        assert_eq!(state, VmState::HALT);
        assert!(!ok, "a scoped Notary signer must be rejected");

        // Scope None on the Notary signer passes the check.
        let tx2 = notary_assisted_tx(vec![
            Signer::new(payer, WitnessScope::NONE),
            Signer::new(Notary::script_hash(), WitnessScope::NONE),
        ]);
        let signature2 = sign_tx(&tx2, &private_key);
        let container2: Arc<dyn Verifiable> = Arc::new(tx2);
        let (state2, ok2) = call_verify(snapshot, Some(container2), Some(&signature2));
        assert_eq!(state2, VmState::HALT);
        assert!(ok2, "a scope-None Notary signer must pass");
    }

    #[test]
    fn verify_notary_paid_transactions_require_a_funding_deposit() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        let payer = UInt160::from_bytes(&[0x06; 20]).unwrap();

        // Sender == Notary (fees paid from the payer's deposit): SystemFee +
        // NetworkFee = 10 must be covered by the deposit.
        let mut tx = notary_assisted_tx(vec![
            Signer::new(Notary::script_hash(), WitnessScope::NONE),
            Signer::new(payer, WitnessScope::NONE),
        ]);
        tx.set_system_fee(6);
        tx.set_network_fee(4);
        let signature = sign_tx(&tx, &private_key);
        let container: Arc<dyn Verifiable> = Arc::new(tx);

        // No deposit -> false.
        let snapshot = seeded_snapshot(&[pubkey]);
        let (state, ok) = call_verify(
            Arc::clone(&snapshot),
            Some(Arc::clone(&container)),
            Some(&signature),
        );
        assert_eq!(state, VmState::HALT);
        assert!(
            !ok,
            "a Notary-paid tx without a payer deposit must be false"
        );

        // An underfunded deposit (9 < 10) -> false.
        Notary::new()
            .write_deposit(&snapshot, &payer, &BigInt::from(9), 1000)
            .unwrap();
        let (state2, ok2) = call_verify(
            Arc::clone(&snapshot),
            Some(Arc::clone(&container)),
            Some(&signature),
        );
        assert_eq!(state2, VmState::HALT);
        assert!(!ok2, "an underfunded deposit must be false");

        // A deposit covering the fees exactly -> true.
        Notary::new()
            .write_deposit(&snapshot, &payer, &BigInt::from(10), 1000)
            .unwrap();
        let (state3, ok3) = call_verify(Arc::clone(&snapshot), Some(container), Some(&signature));
        assert_eq!(state3, VmState::HALT);
        assert!(ok3, "a funded deposit must verify");

        // A single-signer Notary-paid tx (Signers.Length != 2) -> false.
        let mut single =
            notary_assisted_tx(vec![Signer::new(Notary::script_hash(), WitnessScope::NONE)]);
        single.set_system_fee(6);
        single.set_network_fee(4);
        let sig_single = sign_tx(&single, &private_key);
        let container_single: Arc<dyn Verifiable> = Arc::new(single);
        let (state4, ok4) = call_verify(snapshot, Some(container_single), Some(&sig_single));
        assert_eq!(state4, VmState::HALT);
        assert!(!ok4, "Sender == Notary requires exactly two signers");
    }

    /// C# `Notary.OnManifestCompose` (Notary.cs:92-102): NEP-27 alone until
    /// HF_Faun is enabled at the height, then NEP-27 + NEP-30.
    #[test]
    fn manifest_standards_gain_nep30_at_faun() {
        let echidna_only = build_native_contract_state(&Notary, &echidna_settings(), 0);
        assert_eq!(echidna_only.manifest.supported_standards, ["NEP-27"]);

        let mut settings = echidna_settings();
        settings.hardforks.insert(Hardfork::HfFaun, 10);
        let before = build_native_contract_state(&Notary, &settings, 9);
        assert_eq!(before.manifest.supported_standards, ["NEP-27"]);
        let after = build_native_contract_state(&Notary, &settings, 10);
        assert_eq!(after.manifest.supported_standards, ["NEP-27", "NEP-30"]);
    }

    /// Reads the GAS balance of `account` out of the NEP-17 account record
    /// (`Struct[Integer(balance), ...]`), returning 0 when absent.
    fn gas_balance(snapshot: &DataCache, account: &UInt160) -> BigInt {
        let mut key = vec![crate::NEP17_PREFIX_ACCOUNT];
        key.extend_from_slice(&account.to_bytes());
        match snapshot.get(&StorageKey::new(crate::GasToken::ID, key)) {
            Some(item) => {
                let st = BinarySerializer::deserialize(
                    &item.value_bytes(),
                    &ExecutionEngineLimits::default(),
                    None,
                )
                .expect("decode NEP-17 account record");
                match st {
                    StackItem::Struct(fields) => {
                        fields.items()[0].as_int().expect("balance integer")
                    }
                    _ => BigInt::from(0),
                }
            }
            None => BigInt::from(0),
        }
    }

    /// C# `Notary.OnPersistAsync` (Notary.cs:61-90): a NotaryAssisted
    /// transaction paid by the Notary debits the payer's deposit by
    /// `SystemFee + NetworkFee`, and the per-notary reward `(nKeys + 1) *
    /// GetAttributeFeeV1(NotaryAssisted) / notaries.Length` is minted to each
    /// designated P2PNotary node. This is the reminting counterpart of the
    /// NotaryAssisted share `GasToken::on_persist` withholds from the primary
    /// network-fee mint.
    #[test]
    fn on_persist_debits_payer_deposit_and_mints_notary_reward() {
        let private_key = Secp256r1Crypto::generate_private_key();
        let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
        // Deploys Notary + designates one P2PNotary node effective from 0.
        let snapshot = seeded_snapshot(&[pubkey]);

        // Seed the Policy NotaryAssisted attribute fee (HF_Echidna default,
        // 0.1 GAS), the value `GetAttributeFeeV1` reads.
        const FEE: i64 = 1000_0000;
        snapshot.add(
            StorageKey::new(
                crate::PolicyContract::ID,
                vec![20u8, TransactionAttributeType::NotaryAssisted.to_byte()],
            ),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(FEE))),
        );

        // Seed the payer's deposit (amount D, till T).
        let payer = UInt160::from_bytes(&[0x07; 20]).unwrap();
        let deposit_amount = BigInt::from(5_0000_0000i64); // 5 GAS
        Notary::new()
            .write_deposit(&snapshot, &payer, &deposit_amount, 1000)
            .unwrap();

        // A NotaryAssisted tx (nKeys = 1) paid by the Notary on behalf of the
        // payer: Signers = [Notary, payer].
        let notary_hash = Notary::script_hash();
        let mut tx = notary_assisted_tx(vec![
            Signer::new(notary_hash, WitnessScope::NONE),
            Signer::new(payer, WitnessScope::NONE),
        ]);
        tx.set_system_fee(1_0000_0000); // 1 GAS
        tx.set_network_fee(5000_0000); // 0.5 GAS
        let fees = tx.system_fee().wrapping_add(tx.network_fee());

        let mut header = Header::new();
        header.set_index(1);
        let block = Block::from_parts(header, vec![tx]);

        let mut engine = ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            Arc::clone(&snapshot),
            Some(block),
            echidna_settings(),
            0,
            None,
        )
        .expect("engine builds");
        NativeContract::on_persist(&Notary, &mut engine).expect("notary on_persist");

        // Payer deposit debited by SystemFee + NetworkFee; Till unchanged.
        let (amount_after, till_after) = Notary::new()
            .read_deposit(&snapshot, &payer)
            .expect("deposit read")
            .expect("deposit present");
        assert_eq!(amount_after, &deposit_amount - BigInt::from(fees));
        assert_eq!(till_after, 1000);

        // Reward minted to the single designated notary: nFees = nKeys + 1 = 2,
        // singleReward = 2 * FEE / 1.
        let notaries = RoleManagement::new()
            .get_designated_by_role_at(&snapshot, Role::P2PNotary, 1)
            .unwrap();
        assert_eq!(notaries.len(), 1);
        let notary_addr = UInt160::from_script(&Contract::create_signature_redeem_script(
            notaries[0].clone(),
        ));
        assert_eq!(gas_balance(&snapshot, &notary_addr), BigInt::from(2 * FEE));
    }
}
