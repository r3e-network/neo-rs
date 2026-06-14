//! GasToken (GAS) native contract (id -6).
//!
//! Implements the NEP-17 metadata of the C# `Neo.SmartContract.Native.GasToken`
//! (`symbol` "GAS", `decimals` 8). The stateful NEP-17 methods (`totalSupply`,
//! `balanceOf`, `transfer`) are the next increment on the storage-backed
//! pattern; the methods declared below are byte-for-byte C# parity.

use std::any::Any;
use std::sync::LazyLock;

use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, Contract, NativeContract, NativeEvent, NativeMethod};
use neo_payloads::TransactionAttribute;
use neo_primitives::{CallFlags, ContractParameterType, TransactionAttributeType, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::Zero;

use crate::hashes::GAS_TOKEN_HASH;

/// The balance outcome of a GAS transfer (no governance — `OnBalanceChanging` is
/// a no-op for GAS).
#[derive(Debug, PartialEq, Eq)]
enum GasTransferOutcome {
    /// `amount > 0` but the from-account is absent or under-funded: return `false`,
    /// no state change.
    InsufficientBalance,
    /// No balance movement (`amount == 0`, or `from == to`): succeed and still emit,
    /// but write no balances.
    NoMovement,
    /// Deduct `amount` from `from` (delete its entry when `delete_from`) and credit
    /// `to` with `to_new`.
    Move {
        from_new: BigInt,
        delete_from: bool,
        to_new: BigInt,
    },
}

/// The GasToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct GasToken;

impl GasToken {
    /// Stable native contract id (matches C# `GasToken`).
    pub const ID: i32 = -6;
    /// Stable native contract name (matches C# `GasToken.Name`).
    pub const NAME: &'static str = "GasToken";
    /// NEP-17 symbol (C# `GasToken.Symbol => "GAS"`).
    pub const SYMBOL: &'static str = "GAS";
    /// NEP-17 decimals (C# `GasToken.Decimals => 8`).
    pub const DECIMALS: u8 = 8;

    /// Construct a new `GasToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the GAS script hash.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the GAS script hash.
    pub fn script_hash() -> UInt160 {
        *GAS_TOKEN_HASH
    }

    /// The GAS account storage key `(GasToken.ID, [Prefix_Account, account])`.
    fn gas_account_key(account: &UInt160) -> StorageKey {
        StorageKey::new(
            GasToken::ID,
            crate::keys::prefixed_with_hash160(crate::NEP17_PREFIX_ACCOUNT, account),
        )
    }

    /// Reads the GAS account balance, or `None` when the account has no entry. The
    /// GAS account state is the base `FungibleToken.AccountState` = `Struct[Balance]`
    /// (a single field), so `read_nep17_balance`'s field 0 is the balance.
    fn read_gas_account(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
    ) -> CoreResult<Option<BigInt>> {
        let Some(item) = snapshot.get(&Self::gas_account_key(account)) else {
            return Ok(None);
        };
        let state = BinarySerializer::deserialize(
            &item.value_bytes(),
            &ExecutionEngineLimits::default(),
            None,
        )
        .map_err(|e| CoreError::deserialization(format!("GAS account state: {e}")))?;
        let StackItem::Struct(fields) = state else {
            return Err(CoreError::invalid_data("GAS account state is not a struct"));
        };
        let balance = fields
            .items()
            .first()
            .ok_or_else(|| CoreError::invalid_data("GAS account Balance missing"))?
            .as_int()
            .map_err(|e| CoreError::invalid_data(format!("GAS account Balance: {e}")))?;
        Ok(Some(balance))
    }

    /// Writes the GAS account state `Struct[Balance]` (C# `GetAndChange(...).Set`).
    fn write_gas_account(
        &self,
        snapshot: &DataCache,
        account: &UInt160,
        balance: &BigInt,
    ) -> CoreResult<()> {
        let item = StackItem::from_struct(vec![StackItem::from_int(balance.clone())]);
        let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
            .map_err(|e| CoreError::serialization(format!("GAS account serialize: {e}")))?;
        snapshot.update(
            Self::gas_account_key(account),
            StorageItem::from_bytes(bytes),
        );
        Ok(())
    }

    /// Deletes the GAS account entry (C# `Delete(keyFrom)` when a balance reaches 0).
    fn delete_gas_account(&self, snapshot: &DataCache, account: &UInt160) {
        snapshot.delete(&Self::gas_account_key(account));
    }

    /// Pure GAS-transfer balance arithmetic, mirroring C# `FungibleToken.Transfer`
    /// with a no-op `OnBalanceChanging`. `amount` is assumed non-negative (the caller
    /// faults on negative). `from_balance`/`to_balance` are the current balances
    /// (`None` = no account entry).
    fn compute_gas_transfer(
        from_balance: Option<BigInt>,
        to_balance: Option<BigInt>,
        same_account: bool,
        amount: &BigInt,
    ) -> GasTransferOutcome {
        if amount.is_zero() {
            // C#: amount == 0 -> OnBalanceChanging no-op; no balance movement.
            return GasTransferOutcome::NoMovement;
        }
        let Some(from_bal) = from_balance else {
            return GasTransferOutcome::InsufficientBalance; // storageFrom is null
        };
        if &from_bal < amount {
            return GasTransferOutcome::InsufficientBalance;
        }
        if same_account {
            // C#: from == to -> OnBalanceChanging(0); no net movement.
            return GasTransferOutcome::NoMovement;
        }
        let from_new = &from_bal - amount;
        let delete_from = from_new.is_zero(); // C#: if stateFrom.Balance == amount -> Delete.
        let to_new = to_balance.unwrap_or_else(BigInt::zero) + amount;
        GasTransferOutcome::Move {
            from_new,
            delete_from,
            to_new,
        }
    }

    /// C# `FungibleToken.PostTransferAsync` for the transfer case (both `from`/`to`
    /// non-null): emit the `Transfer` event, then — only when `to` is a deployed
    /// contract — queue its `onNEP17Payment(from, amount, data)` callback (run after
    /// this native call returns, faithful to `CallFromNativeContractAsync`).
    fn post_transfer(
        &self,
        engine: &mut ApplicationEngine,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
        data: &[u8],
    ) -> CoreResult<()> {
        let gas_hash = GasToken::script_hash();
        engine
            .send_notification(
                gas_hash,
                "Transfer".to_string(),
                vec![
                    StackItem::from_byte_string(from.to_bytes()),
                    StackItem::from_byte_string(to.to_bytes()),
                    StackItem::from_int(amount.clone()),
                ],
            )
            .map_err(|e| CoreError::invalid_operation(format!("GasToken::transfer notify: {e}")))?;

        let is_contract = crate::ContractManagement::is_contract(&engine.snapshot_cache(), to);
        if !is_contract {
            return Ok(());
        }
        // The transfer `data` param is `Any`, so it arrives BinarySerialized; round-trip
        // it back to the StackItem passed to onNEP17Payment.
        let data_item = if data.is_empty() {
            StackItem::null()
        } else {
            BinarySerializer::deserialize(data, &ExecutionEngineLimits::default(), None)
                .map_err(|e| CoreError::deserialization(format!("GasToken::transfer data: {e}")))?
        };
        engine.queue_contract_call_from_native(
            gas_hash,
            *to,
            "onNEP17Payment",
            vec![
                StackItem::from_byte_string(from.to_bytes()),
                StackItem::from_int(amount.clone()),
                data_item,
            ],
        );
        Ok(())
    }

    /// C# `GasToken.Mint` (`FungibleToken.Mint`): credits `amount` GAS to `account`,
    /// raises the total supply, and emits `Transfer(null, account, amount)` —
    /// queuing the recipient's `onNEP17Payment` when `call_on_payment` and the
    /// recipient is a deployed contract. A zero amount is a no-op; a negative amount
    /// faults. `pub(crate)` so NeoToken's reward distribution can mint GAS.
    pub(crate) fn gas_mint(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        amount: &BigInt,
        call_on_payment: bool,
    ) -> CoreResult<()> {
        if amount < &BigInt::zero() {
            return Err(CoreError::invalid_operation(
                "GasToken::mint: amount cannot be negative",
            ));
        }
        if amount.is_zero() {
            return Ok(());
        }
        let snapshot = engine.snapshot_cache();
        let balance = self
            .read_gas_account(&snapshot, account)?
            .unwrap_or_else(BigInt::zero)
            + amount;
        self.write_gas_account(&snapshot, account, &balance)?;
        let supply_key = StorageKey::new(GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]);
        let supply = snapshot
            .get(&supply_key)
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(BigInt::zero)
            + amount;
        snapshot.update(
            supply_key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)),
        );
        self.post_mint(engine, account, amount, call_on_payment)
    }

    /// C# `PostTransferAsync(null, account, amount, …)` for the mint case: emit
    /// `Transfer(null, account, amount)`, then queue `onNEP17Payment(null, amount,
    /// null)` when `call_on_payment` and the recipient is a deployed contract.
    fn post_mint(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        amount: &BigInt,
        call_on_payment: bool,
    ) -> CoreResult<()> {
        let gas_hash = GasToken::script_hash();
        engine
            .send_notification(
                gas_hash,
                "Transfer".to_string(),
                vec![
                    StackItem::null(),
                    StackItem::from_byte_string(account.to_bytes()),
                    StackItem::from_int(amount.clone()),
                ],
            )
            .map_err(|e| CoreError::invalid_operation(format!("GasToken::mint notify: {e}")))?;
        if !call_on_payment {
            return Ok(());
        }
        if !crate::ContractManagement::is_contract(&engine.snapshot_cache(), account) {
            return Ok(());
        }
        engine.queue_contract_call_from_native(
            gas_hash,
            *account,
            "onNEP17Payment",
            vec![
                StackItem::null(),
                StackItem::from_int(amount.clone()),
                StackItem::null(),
            ],
        );
        Ok(())
    }

    /// C# `GasToken.Burn` (`FungibleToken.Burn`): debits `amount` GAS from
    /// `account` (deleting the entry when the balance reaches zero, like C#'s
    /// `Delete` on `Balance == amount`), lowers the total supply, and emits
    /// `Transfer(account, null, amount)` — no `onNEP17Payment` callback (C# burns
    /// with `callOnPayment: false`). A zero amount is a no-op; a negative amount
    /// or an insufficient balance faults (C# throws on `Balance < amount`, and a
    /// missing account entry NREs on the null-forgiving `GetAndChange`).
    /// `pub(crate)` so NeoToken's Echidna `onNEP17Payment` can burn the
    /// register-price GAS it receives.
    pub(crate) fn gas_burn(
        &self,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        amount: &BigInt,
    ) -> CoreResult<()> {
        if amount < &BigInt::zero() {
            return Err(CoreError::invalid_operation(
                "GasToken::burn: amount cannot be negative",
            ));
        }
        if amount.is_zero() {
            return Ok(());
        }
        let snapshot = engine.snapshot_cache();
        let balance = self
            .read_gas_account(&snapshot, account)?
            .unwrap_or_else(BigInt::zero);
        if &balance < amount {
            return Err(CoreError::invalid_operation(format!(
                "GasToken::burn: insufficient balance {balance} to burn {amount}"
            )));
        }
        if &balance == amount {
            self.delete_gas_account(&snapshot, account);
        } else {
            self.write_gas_account(&snapshot, account, &(&balance - amount))?;
        }
        let supply_key = StorageKey::new(GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]);
        let supply = snapshot
            .get(&supply_key)
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(BigInt::zero)
            - amount;
        snapshot.update(
            supply_key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)),
        );
        engine
            .send_notification(
                GasToken::script_hash(),
                "Transfer".to_string(),
                vec![
                    StackItem::from_byte_string(account.to_bytes()),
                    StackItem::null(),
                    StackItem::from_int(amount.clone()),
                ],
            )
            .map_err(|e| CoreError::invalid_operation(format!("GasToken::burn notify: {e}")))
    }

    /// C# `NativeContract.GAS.BalanceOf(snapshot, account)`: reads the `Balance`
    /// field of the NEP-17 `AccountState` stored under `Prefix_Account + account`
    /// (zero when absent). The single canonical GAS-balance decode, shared by
    /// the mempool fee check and RPC wallet helpers.
    pub fn balance_of(snapshot: &DataCache, account: &UInt160) -> CoreResult<BigInt> {
        crate::read_nep17_balance(snapshot, Self::ID, account)
    }

    /// Core NEP-17 transfer (C# `FungibleToken.Transfer`), shared by the `transfer`
    /// ABI method and by native-to-native callers (e.g. `Notary.withdraw`).
    ///
    /// `caller` is the script hash treated as the immediate caller for the witness
    /// bypass — C# `from.Equals(CallingScriptHash)`. The ABI method passes the
    /// engine's calling script hash; a native contract transferring its own balance
    /// (where C#'s nested call would set `CallingScriptHash` to that contract)
    /// passes its own hash, so the witness check passes. Returns `Ok(true)`/
    /// `Ok(false)` per NEP-17, or `Err` for a negative `amount`.
    pub(crate) fn transfer_core(
        engine: &mut ApplicationEngine,
        caller: UInt160,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
        data: &[u8],
    ) -> CoreResult<bool> {
        // C#: amount.Sign < 0 -> ArgumentOutOfRangeException (fault).
        if amount < &BigInt::zero() {
            return Err(CoreError::invalid_operation(
                "GasToken::transfer: amount cannot be negative",
            ));
        }
        // C#: !from.Equals(CallingScriptHash) && !CheckWitnessInternal(from) -> false.
        let witnessed = from == &caller
            || engine.check_witness(from).map_err(|e| {
                CoreError::invalid_operation(format!("GasToken::transfer witness: {e}"))
            })?;
        if !witnessed {
            return Ok(false);
        }

        let snapshot = engine.snapshot_cache();
        let from_balance = GasToken::new().read_gas_account(&snapshot, from)?;
        let to_balance = GasToken::new().read_gas_account(&snapshot, to)?;
        match Self::compute_gas_transfer(from_balance, to_balance, from == to, amount) {
            GasTransferOutcome::InsufficientBalance => return Ok(false),
            GasTransferOutcome::NoMovement => {}
            GasTransferOutcome::Move {
                from_new,
                delete_from,
                to_new,
            } => {
                if delete_from {
                    GasToken::new().delete_gas_account(&snapshot, from);
                } else {
                    GasToken::new().write_gas_account(&snapshot, from, &from_new)?;
                }
                GasToken::new().write_gas_account(&snapshot, to, &to_new)?;
            }
        }

        // PostTransfer: emit Transfer + (if `to` is a contract) onNEP17Payment.
        GasToken::new().post_transfer(engine, from, to, amount, data)?;
        Ok(true)
    }
}

static GAS_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        NativeMethod::new(
            "symbol".into(),
            0,
            true,
            0,
            vec![],
            ContractParameterType::String,
        ),
        NativeMethod::new(
            "decimals".into(),
            0,
            true,
            0,
            vec![],
            ContractParameterType::Integer,
        ),
        // NEP-17 state reads: CpuFee 1<<15, RequiredCallFlags ReadStates.
        NativeMethod::new(
            "totalSupply".into(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        NativeMethod::new(
            "balanceOf".into(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Integer,
        )
        .with_parameter_names(["account"]),
        // NEP-17 transfer: CpuFee 1<<17, StorageFee 50, States|AllowCall|AllowNotify,
        // (from, to, amount, data) -> Boolean. Not safe.
        NativeMethod::new(
            "transfer".into(),
            1 << 17,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
                ContractParameterType::Integer,
                ContractParameterType::Any,
            ],
            ContractParameterType::Boolean,
        )
        .with_storage_fee(50)
        .with_parameter_names(["from", "to", "amount", "data"]),
    ]
});

/// GAS declares no events of its own; the only manifest event is the
/// `Transfer` inherited from the C# `FungibleToken` base constructor.
static GAS_EVENTS: LazyLock<Vec<NativeEvent>> =
    LazyLock::new(|| vec![crate::fungible_token_transfer_event()]);

impl NativeContract for GasToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &GAS_METHODS
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &GAS_EVENTS
    }

    /// C# `FungibleToken.OnManifestCompose` (FungibleToken.cs:68-71): every
    /// fungible token declares NEP-17 unconditionally.
    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        vec!["NEP-17".to_string()]
    }

    /// C# `GasToken.InitializeAsync(engine, hardfork)` for `hardfork == ActiveIn`
    /// (GasToken.cs:29-37; GAS is genesis-active, so this runs while persisting
    /// block 0): mint `ProtocolSettings.InitialGasDistribution` GAS to the BFT
    /// address of the standby validators, with `callOnPayment: false`.
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let standby_validators = engine.protocol_settings().standby_validators();
        let initial = BigInt::from(engine.protocol_settings().initial_gas_distribution);
        let account = crate::NeoToken::bft_address(&standby_validators)?;
        self.gas_mint(engine, &account, &initial, false)
    }

    /// C# `GasToken.OnPersistAsync` (GasToken.cs:39-58): for every transaction
    /// in the persisting block, burn the sender's `SystemFee + NetworkFee` and
    /// accumulate the network fee into the block total; a `NotaryAssisted`
    /// attribute redirects `(NKeys + 1) * AttributeFee(NotaryAssisted)` of that
    /// total to the designated notary nodes (minted by the Notary contract in
    /// its own `PostPersist`), so it is subtracted here. Finally mint the
    /// remaining total to the primary validator — the signature-contract
    /// address of `NEO.GetNextBlockValidators(...)[block.PrimaryIndex]` — with
    /// `callOnPayment: false`.
    ///
    /// The NotaryAssisted branch is not hardfork-gated in C# (the attribute is
    /// only valid in transactions once HF_Echidna verification admits it, so
    /// the gate is implicit), and `GetAttributeFeeV1` is the plain
    /// `Prefix_AttributeFee` storage read with the NotaryAssisted type allowed
    /// (PolicyContract.cs:278-301).
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        // Collect the per-transaction data under the shared block borrow; the
        // burns below need `&mut engine`.
        let (primary_index, tx_data) = {
            let block = engine.persisting_block().ok_or_else(|| {
                CoreError::invalid_operation("GasToken::on_persist requires a persisting block")
            })?;
            let tx_data: Vec<(Option<UInt160>, i64, i64, Option<u8>)> = block
                .transactions
                .iter()
                .map(|tx| {
                    // C# `tx.GetAttribute<NotaryAssisted>()`: the first (and, by
                    // AllowMultiple=false, only) NotaryAssisted attribute.
                    let nkeys = tx.attributes().iter().find_map(|attr| match attr {
                        TransactionAttribute::NotaryAssisted(na) => Some(na.nkeys),
                        _ => None,
                    });
                    (tx.sender(), tx.system_fee(), tx.network_fee(), nkeys)
                })
                .collect();
            (usize::from(block.primary_index()), tx_data)
        };

        let mut total_network_fee: i64 = 0;
        for (sender, system_fee, network_fee, notary_nkeys) in tx_data {
            // C# `tx.Sender` is `Signers[0].Account`; a signerless transaction
            // cannot appear in a valid block (C# would throw on the indexer).
            let sender = sender.ok_or_else(|| {
                CoreError::invalid_operation("GasToken::on_persist: transaction has no sender")
            })?;
            let fee = system_fee.checked_add(network_fee).ok_or_else(|| {
                CoreError::invalid_operation("GasToken::on_persist: fee overflow")
            })?;
            self.gas_burn(engine, &sender, &BigInt::from(fee))?;
            total_network_fee = total_network_fee.checked_add(network_fee).ok_or_else(|| {
                CoreError::invalid_operation("GasToken::on_persist: network fee overflow")
            })?;
            if let Some(nkeys) = notary_nkeys {
                // C# `(notaryAssisted.NKeys + 1) * Policy.GetAttributeFeeV1(
                // snapshot, (byte)notaryAssisted.Type)`.
                let per_key = crate::PolicyContract::new().attribute_fee(
                    &engine.snapshot_cache(),
                    TransactionAttributeType::NotaryAssisted.to_byte(),
                    true,
                )?;
                total_network_fee -= (i64::from(nkeys) + 1) * per_key;
            }
        }

        // C# `NEO.GetNextBlockValidators(snapshot, settings.ValidatorsCount)`,
        // indexed by the persisting block's PrimaryIndex; an index outside the
        // validator set faults the block (C# IndexOutOfRangeException).
        let validators_count =
            usize::try_from(engine.protocol_settings().validators_count).unwrap_or(0);
        let snapshot = engine.snapshot_cache();
        let validators =
            crate::NeoToken::new().next_block_validators(&snapshot, validators_count)?;
        let primary_key = validators.get(primary_index).ok_or_else(|| {
            CoreError::invalid_operation(format!(
                "GasToken::on_persist: primary index {primary_index} outside the validator set"
            ))
        })?;
        let primary = UInt160::from_script(&Contract::create_signature_redeem_script(
            primary_key.clone(),
        ));
        self.gas_mint(engine, &primary, &BigInt::from(total_network_fee), false)
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
        match method {
            "symbol" => Ok(Self::SYMBOL.as_bytes().to_vec()),
            "decimals" => Ok(BigInt::from(Self::DECIMALS).to_signed_bytes_le()),
            "totalSupply" => {
                let snapshot = engine.snapshot_cache();
                let total = crate::read_storage_int(
                    &snapshot,
                    Self::ID,
                    crate::NEP17_PREFIX_TOTAL_SUPPLY,
                    0,
                )?;
                Ok(BigInt::from(total).to_signed_bytes_le())
            }
            "balanceOf" => {
                let account = crate::args::raw_account(args, "GasToken::balanceOf")?;
                let snapshot = engine.snapshot_cache();
                Ok(crate::read_nep17_balance(&snapshot, Self::ID, &account)?.to_signed_bytes_le())
            }
            "transfer" => {
                // C# FungibleToken.Transfer(from, to, amount, data).
                let from = crate::args::raw_hash160(args, 0, "GasToken::transfer")?;
                let to = crate::args::raw_hash160(args, 1, "GasToken::transfer")?;
                let amount = BigInt::from_signed_bytes_le(args.get(2).ok_or_else(|| {
                    CoreError::invalid_operation("GasToken::transfer requires an amount")
                })?);
                let data = args.get(3).map(Vec::as_slice).unwrap_or(&[]);
                // The witness bypass uses the engine's calling script hash
                // (C# `from.Equals(CallingScriptHash)`).
                let caller = engine
                    .get_calling_script_hash()
                    .unwrap_or_else(UInt160::zero);
                Ok(vec![u8::from(Self::transfer_core(
                    engine, caller, &from, &to, &amount, data,
                )?)])
            }
            other => Err(CoreError::invalid_operation(format!(
                "GasToken method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_contract_surface() {
        let c = GasToken::new();
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            ["symbol", "decimals", "totalSupply", "balanceOf", "transfer"]
        );
        // Metadata getters are zero-fee; the state reads are ReadStates getters.
        let symbol = c.methods().iter().find(|m| m.name == "symbol").unwrap();
        assert!(symbol.safe && symbol.cpu_fee == 0 && symbol.required_call_flags == 0);
        let balance = c.methods().iter().find(|m| m.name == "balanceOf").unwrap();
        assert_eq!(balance.required_call_flags, CallFlags::READ_STATES.bits());
        // transfer: not safe, States|AllowCall|AllowNotify, StorageFee 50,
        // (Hash160, Hash160, Integer, Any) -> Boolean.
        let transfer = c.methods().iter().find(|m| m.name == "transfer").unwrap();
        assert!(!transfer.safe);
        assert_eq!(transfer.cpu_fee, 1 << 17);
        assert_eq!(transfer.storage_fee, 50);
        assert_eq!(
            transfer.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert_eq!(
            transfer.parameters,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
                ContractParameterType::Integer,
                ContractParameterType::Any
            ]
        );
        assert_eq!(transfer.return_type, ContractParameterType::Boolean);
    }

    #[test]
    fn compute_gas_transfer_matches_csharp_balance_arithmetic() {
        let amt = BigInt::from(100);

        // amount == 0 -> no movement (succeeds, emits) regardless of balances.
        assert_eq!(
            GasToken::compute_gas_transfer(None, None, false, &BigInt::zero()),
            GasTransferOutcome::NoMovement
        );

        // amount > 0, from has no account -> insufficient.
        assert_eq!(
            GasToken::compute_gas_transfer(None, None, false, &amt),
            GasTransferOutcome::InsufficientBalance
        );
        // amount > 0, from underfunded -> insufficient.
        assert_eq!(
            GasToken::compute_gas_transfer(Some(BigInt::from(99)), None, false, &amt),
            GasTransferOutcome::InsufficientBalance
        );
        // from == to with sufficient balance -> no movement.
        assert_eq!(
            GasToken::compute_gas_transfer(
                Some(BigInt::from(100)),
                Some(BigInt::from(100)),
                true,
                &amt
            ),
            GasTransferOutcome::NoMovement
        );
        // Exact balance -> deduct to zero deletes the from-entry; to credited.
        assert_eq!(
            GasToken::compute_gas_transfer(Some(BigInt::from(100)), None, false, &amt),
            GasTransferOutcome::Move {
                from_new: BigInt::zero(),
                delete_from: true,
                to_new: BigInt::from(100)
            }
        );
        // Partial balance -> from keeps the remainder; existing to is added to.
        assert_eq!(
            GasToken::compute_gas_transfer(
                Some(BigInt::from(250)),
                Some(BigInt::from(7)),
                false,
                &amt
            ),
            GasTransferOutcome::Move {
                from_new: BigInt::from(150),
                delete_from: false,
                to_new: BigInt::from(107)
            }
        );
    }

    #[test]
    fn gas_account_storage_round_trips() {
        use neo_storage::persistence::DataCache;
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[3u8; 20]).unwrap();

        assert!(
            GasToken::new()
                .read_gas_account(&cache, &account)
                .unwrap()
                .is_none()
        );
        GasToken::new()
            .write_gas_account(&cache, &account, &BigInt::from(12345))
            .unwrap();
        assert_eq!(
            GasToken::new().read_gas_account(&cache, &account).unwrap(),
            Some(BigInt::from(12345))
        );
        // The single-field Struct[Balance] layout is what read_nep17_balance reads.
        assert_eq!(
            crate::read_nep17_balance(&cache, GasToken::ID, &account).unwrap(),
            BigInt::from(12345)
        );
        GasToken::new().delete_gas_account(&cache, &account);
        assert!(
            GasToken::new()
                .read_gas_account(&cache, &account)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn balance_of_absent_account_is_zero() {
        use neo_storage::persistence::DataCache;
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[1u8; 20]).unwrap();
        // C# BalanceOf returns BigInteger.Zero when the account has no entry.
        assert_eq!(
            crate::read_nep17_balance(&cache, GasToken::ID, &account).unwrap(),
            BigInt::from(0)
        );
    }

    /// C# `FungibleToken.OnManifestCompose` (FungibleToken.cs:68-71): the
    /// generated GAS manifest declares NEP-17 regardless of the hardfork
    /// configuration or height.
    #[test]
    fn manifest_declares_nep17() {
        use neo_execution::native_contract::build_native_contract_state;

        let state = build_native_contract_state(&GasToken, &ProtocolSettings::default(), 0);
        assert_eq!(state.manifest.supported_standards, ["NEP-17"]);
        let later = build_native_contract_state(&GasToken, &ProtocolSettings::default(), u32::MAX);
        assert_eq!(later.manifest.supported_standards, ["NEP-17"]);
    }

    /// C# `FungibleToken.Burn`: a negative amount faults, zero is a no-op, an
    /// under-funded account faults, a partial burn debits balance and supply
    /// (emitting `Transfer(account, null, amount)`), and a full burn deletes
    /// the account entry.
    #[test]
    fn gas_burn_debits_balance_and_supply() {
        use neo_primitives::TriggerType;
        use std::sync::Arc;

        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[9u8; 20]).unwrap();
        GasToken::new()
            .write_gas_account(&cache, &account, &BigInt::from(100))
            .unwrap();
        let supply_key = StorageKey::new(GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]);
        cache.add(
            supply_key.clone(),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(100))),
        );
        let snapshot = Arc::new(cache);
        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            10_000_000,
            None,
        )
        .expect("engine builds");

        // Negative -> fault; zero -> no-op (no event, no state change).
        assert!(
            GasToken::new()
                .gas_burn(&mut engine, &account, &BigInt::from(-1))
                .is_err()
        );
        GasToken::new()
            .gas_burn(&mut engine, &account, &BigInt::from(0))
            .unwrap();
        assert!(engine.notifications().is_empty());
        assert_eq!(
            GasToken::new()
                .read_gas_account(&snapshot, &account)
                .unwrap(),
            Some(BigInt::from(100))
        );

        // Partial burn: balance and supply shrink, Transfer(account, null, 30).
        GasToken::new()
            .gas_burn(&mut engine, &account, &BigInt::from(30))
            .unwrap();
        assert_eq!(
            GasToken::new()
                .read_gas_account(&snapshot, &account)
                .unwrap(),
            Some(BigInt::from(70))
        );
        assert_eq!(
            BigInt::from_signed_bytes_le(&snapshot.get(&supply_key).unwrap().value_bytes()),
            BigInt::from(70)
        );
        assert_eq!(engine.notifications().len(), 1);

        // Over-burn -> fault, state unchanged.
        assert!(
            GasToken::new()
                .gas_burn(&mut engine, &account, &BigInt::from(71))
                .is_err()
        );
        assert_eq!(
            GasToken::new()
                .read_gas_account(&snapshot, &account)
                .unwrap(),
            Some(BigInt::from(70))
        );

        // Full burn deletes the account entry; the supply reaches zero (stored
        // as the canonical empty-bytes BigInteger).
        GasToken::new()
            .gas_burn(&mut engine, &account, &BigInt::from(70))
            .unwrap();
        assert!(
            GasToken::new()
                .read_gas_account(&snapshot, &account)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            BigInt::from_signed_bytes_le(&snapshot.get(&supply_key).unwrap().value_bytes()),
            BigInt::from(0)
        );
    }
}

/// `GasToken::initialize` and `GasToken::on_persist` against the C# oracle
/// (GasToken.cs:29-58): the genesis InitialGasDistribution mint, the
/// per-transaction fee burns, the network-fee mint to the primary validator,
/// and the NotaryAssisted attribute deduction.
#[cfg(test)]
mod persist_tests {
    use super::*;
    use std::sync::Arc;

    use neo_crypto::ECPoint;
    use neo_payloads::{Block, Header, NotaryAssisted, Signer, Transaction};
    use neo_primitives::{TriggerType, WitnessScope};

    use crate::test_support::{
        NEO_PREFIX_COMMITTEE, POLICY_PREFIX_ATTRIBUTE_FEE, hex, sample_committee, seed_committee,
    };

    /// The signature-contract address of `points[primary]` after the C#
    /// `GetNextBlockValidators` ordering (take ValidatorsCount, sort ascending).
    fn primary_address(points: &[ECPoint], validators_count: usize, primary: usize) -> UInt160 {
        let mut sorted: Vec<ECPoint> = points.iter().take(validators_count).cloned().collect();
        sorted.sort();
        UInt160::from_script(&Contract::create_signature_redeem_script(
            sorted[primary].clone(),
        ))
    }

    fn fee_tx(sender: UInt160, system_fee: i64, network_fee: i64) -> Transaction {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(sender, WitnessScope::NONE)]);
        tx.set_system_fee(system_fee);
        tx.set_network_fee(network_fee);
        tx
    }

    fn on_persist_engine(snapshot: Arc<DataCache>, block: Block) -> ApplicationEngine {
        // C# runs the native OnPersist script with gas limit 0.
        ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            snapshot,
            Some(block),
            ProtocolSettings::default(),
            0,
            None,
        )
        .expect("engine builds")
    }

    fn seed_gas(cache: &DataCache, account: &UInt160, balance: i64) {
        GasToken::new()
            .write_gas_account(cache, account, &BigInt::from(balance))
            .unwrap();
        let supply_key = StorageKey::new(GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]);
        let supply = cache
            .get(&supply_key)
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
            .unwrap_or_else(BigInt::zero)
            + BigInt::from(balance);
        cache.update(
            supply_key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)),
        );
    }

    fn balance(cache: &DataCache, account: &UInt160) -> BigInt {
        crate::read_nep17_balance(cache, GasToken::ID, account).unwrap()
    }

    /// C# `GasToken.InitializeAsync` (GasToken.cs:29-37): the genesis pass
    /// mints `InitialGasDistribution` (52M GAS) to the BFT address of the
    /// standby validators and emits `Transfer(null, bft, amount)`.
    #[test]
    fn initialize_mints_initial_gas_distribution_to_bft_address() {
        let settings = ProtocolSettings::default();
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            Arc::clone(&snapshot),
            None,
            settings.clone(),
            0,
            None,
        )
        .expect("engine builds");

        NativeContract::initialize(&GasToken::new(), &mut engine).expect("initialize");

        let bft = crate::NeoToken::bft_address(&settings.standby_validators()).unwrap();
        let expected = BigInt::from(settings.initial_gas_distribution);
        assert_eq!(
            balance(&snapshot, &bft),
            expected,
            "52M GAS to the BFT address"
        );
        let supply_key = StorageKey::new(GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]);
        assert_eq!(
            BigInt::from_signed_bytes_le(&snapshot.get(&supply_key).unwrap().value_bytes()),
            expected,
            "total supply equals the initial distribution"
        );
        assert_eq!(engine.notifications().len(), 1);
        let transfer = &engine.notifications()[0];
        assert_eq!(transfer.event_name, "Transfer");
        assert_eq!(transfer.script_hash, GasToken::script_hash());
        assert!(
            matches!(transfer.state[0], StackItem::Null),
            "from = null (mint)"
        );
        assert_eq!(transfer.state[1].as_bytes().unwrap(), bft.to_bytes());
        assert_eq!(transfer.state[2].as_int().unwrap(), expected);
    }

    /// C# `GasToken.OnPersistAsync` (GasToken.cs:39-58): each sender is burned
    /// `SystemFee + NetworkFee`; the summed network fees are minted to the
    /// primary validator's signature address (validators sorted ascending,
    /// indexed by the block's PrimaryIndex).
    #[test]
    fn on_persist_burns_fees_and_mints_network_fees_to_primary() {
        let settings = ProtocolSettings::default();
        let validators_count = usize::try_from(settings.validators_count).unwrap();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        let sender_a = UInt160::from_bytes(&[0xA1; 20]).unwrap();
        let sender_b = UInt160::from_bytes(&[0xB2; 20]).unwrap();
        seed_gas(&cache, &sender_a, 10_0000_0000);
        seed_gas(&cache, &sender_b, 5_0000_0000);
        let snapshot = Arc::new(cache);

        let mut header = Header::new();
        header.set_index(1);
        header.set_primary_index(1);
        let block = Block::from_parts(
            header,
            vec![
                fee_tx(sender_a, 3_0000_0000, 1_0000_0000),
                fee_tx(sender_b, 2_0000_0000, 5000_0000),
            ],
        );
        let mut engine = on_persist_engine(Arc::clone(&snapshot), block);
        NativeContract::on_persist(&GasToken::new(), &mut engine).expect("on_persist");

        // Burns: sender_a 4 GAS of 10, sender_b 2.5 GAS of 5.
        assert_eq!(balance(&snapshot, &sender_a), BigInt::from(6_0000_0000i64));
        assert_eq!(balance(&snapshot, &sender_b), BigInt::from(2_5000_0000i64));
        // Mint: 1.5 GAS total network fees to the primary validator
        // (sorted validator index 1).
        let primary = primary_address(&committee, validators_count, 1);
        assert_eq!(balance(&snapshot, &primary), BigInt::from(1_5000_0000i64));
        // Supply: 15 GAS seeded - 6.5 burned + 1.5 minted = 10.
        let supply_key = StorageKey::new(GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]);
        assert_eq!(
            BigInt::from_signed_bytes_le(&snapshot.get(&supply_key).unwrap().value_bytes()),
            BigInt::from(10_0000_0000i64)
        );
        // Notifications: Transfer(a, null, 4), Transfer(b, null, 2.5),
        // Transfer(null, primary, 1.5) — burn, burn, mint, in C# order.
        let events: Vec<(bool, bool, BigInt)> = engine
            .notifications()
            .iter()
            .map(|n| {
                (
                    matches!(n.state[0], StackItem::Null),
                    matches!(n.state[1], StackItem::Null),
                    n.state[2].as_int().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            events,
            vec![
                (false, true, BigInt::from(4_0000_0000i64)),
                (false, true, BigInt::from(2_5000_0000i64)),
                (true, false, BigInt::from(1_5000_0000i64)),
            ]
        );
    }

    /// C# `Burn` throws on an under-funded sender (`Balance < amount` ->
    /// InvalidOperationException), faulting the whole block.
    #[test]
    fn on_persist_faults_when_a_sender_cannot_pay_its_fees() {
        let cache = DataCache::new(false);
        seed_committee(&cache, &sample_committee());
        let sender = UInt160::from_bytes(&[0xC3; 20]).unwrap();
        seed_gas(&cache, &sender, 100);
        let snapshot = Arc::new(cache);

        let mut header = Header::new();
        header.set_index(1);
        header.set_primary_index(0);
        let block = Block::from_parts(header, vec![fee_tx(sender, 200, 0)]);
        let mut engine = on_persist_engine(snapshot, block);
        assert!(NativeContract::on_persist(&GasToken::new(), &mut engine).is_err());
    }

    /// C# GasToken.cs:47-53: a NotaryAssisted transaction deducts
    /// `(NKeys + 1) * GetAttributeFeeV1(NotaryAssisted)` from the primary's
    /// mint (the deducted share is minted to notary nodes by the Notary
    /// contract instead).
    #[test]
    fn on_persist_deducts_notary_assisted_share_from_the_primary_mint() {
        let settings = ProtocolSettings::default();
        let validators_count = usize::try_from(settings.validators_count).unwrap();
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        // The Echidna-default NotaryAssisted attribute fee: 0.1 GAS per key.
        cache.add(
            StorageKey::new(
                crate::PolicyContract::ID,
                vec![
                    POLICY_PREFIX_ATTRIBUTE_FEE,
                    neo_primitives::TransactionAttributeType::NotaryAssisted.to_byte(),
                ],
            ),
            StorageItem::from_bytes(BigInt::from(1000_0000i64).to_signed_bytes_le()),
        );
        let sender = UInt160::from_bytes(&[0xD4; 20]).unwrap();
        seed_gas(&cache, &sender, 10_0000_0000);
        let snapshot = Arc::new(cache);

        let mut tx = fee_tx(sender, 1_0000_0000, 2_0000_0000);
        tx.set_attributes(vec![TransactionAttribute::NotaryAssisted(
            NotaryAssisted::new(2),
        )]);
        let mut header = Header::new();
        header.set_index(1);
        header.set_primary_index(0);
        let block = Block::from_parts(header, vec![tx]);
        let mut engine = on_persist_engine(Arc::clone(&snapshot), block);
        NativeContract::on_persist(&GasToken::new(), &mut engine).expect("on_persist");

        // Burn untouched by the attribute: 3 GAS off the sender.
        assert_eq!(balance(&snapshot, &sender), BigInt::from(7_0000_0000i64));
        // Mint: 2 GAS network fee - (2 + 1) * 0.1 GAS = 1.7 GAS.
        let primary = primary_address(&committee, validators_count, 0);
        assert_eq!(balance(&snapshot, &primary), BigInt::from(1_7000_0000i64));
    }

    /// An empty block burns nothing and mints nothing (C# `Mint` returns early
    /// on a zero amount), but still resolves the validator set.
    #[test]
    fn on_persist_is_a_value_noop_for_an_empty_block() {
        let cache = DataCache::new(false);
        let committee = sample_committee();
        seed_committee(&cache, &committee);
        let snapshot = Arc::new(cache);

        let mut header = Header::new();
        header.set_index(2);
        header.set_primary_index(0);
        let block = Block::from_parts(header, Vec::new());
        let mut engine = on_persist_engine(Arc::clone(&snapshot), block);
        NativeContract::on_persist(&GasToken::new(), &mut engine).expect("on_persist");
        assert!(
            engine.notifications().is_empty(),
            "no Transfer for a zero mint"
        );
        let supply_key = StorageKey::new(GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]);
        assert!(snapshot.get(&supply_key).is_none(), "supply untouched");
    }

    /// C# indexes `validators[block.PrimaryIndex]`: an index outside the
    /// validator set is an IndexOutOfRangeException (block fault).
    #[test]
    fn on_persist_faults_on_a_primary_index_outside_the_validator_set() {
        let cache = DataCache::new(false);
        seed_committee(&cache, &sample_committee());
        let snapshot = Arc::new(cache);

        let mut header = Header::new();
        header.set_index(1);
        header.set_primary_index(200);
        let block = Block::from_parts(header, Vec::new());
        let mut engine = on_persist_engine(snapshot, block);
        assert!(NativeContract::on_persist(&GasToken::new(), &mut engine).is_err());
    }
}
