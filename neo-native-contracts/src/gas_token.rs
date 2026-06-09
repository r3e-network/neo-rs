//! GasToken (GAS) native contract (id -6).
//!
//! Implements the NEP-17 metadata of the C# `Neo.SmartContract.Native.GasToken`
//! (`symbol` "GAS", `decimals` 8). The stateful NEP-17 methods (`totalSupply`,
//! `balanceOf`, `transfer`) are the next increment on the storage-backed
//! pattern; the methods declared below are byte-for-byte C# parity.

use std::any::Any;
use std::sync::LazyLock;

use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, NativeContract, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::ExecutionEngineLimits;
use num_bigint::BigInt;
use num_traits::Zero;

use crate::hashes::GAS_TOKEN_HASH;

/// The GAS account storage key `(GasToken.ID, [Prefix_Account, account])`.
fn gas_account_key(account: &UInt160) -> StorageKey {
    let mut key = vec![crate::NEP17_PREFIX_ACCOUNT];
    key.extend_from_slice(&account.to_bytes());
    StorageKey::new(GasToken::ID, key)
}

/// Reads the GAS account balance, or `None` when the account has no entry. The
/// GAS account state is the base `FungibleToken.AccountState` = `Struct[Balance]`
/// (a single field), so `read_nep17_balance`'s field 0 is the balance.
fn read_gas_account(snapshot: &DataCache, account: &UInt160) -> CoreResult<Option<BigInt>> {
    let Some(item) = snapshot.get(&gas_account_key(account)) else {
        return Ok(None);
    };
    let state =
        BinarySerializer::deserialize(&item.value_bytes(), &ExecutionEngineLimits::default(), None)
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
fn write_gas_account(snapshot: &DataCache, account: &UInt160, balance: &BigInt) -> CoreResult<()> {
    let item = StackItem::from_struct(vec![StackItem::from_int(balance.clone())]);
    let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default())
        .map_err(|e| CoreError::serialization(format!("GAS account serialize: {e}")))?;
    snapshot.update(gas_account_key(account), StorageItem::from_bytes(bytes));
    Ok(())
}

/// Deletes the GAS account entry (C# `Delete(keyFrom)` when a balance reaches 0).
fn delete_gas_account(snapshot: &DataCache, account: &UInt160) {
    snapshot.delete(&gas_account_key(account));
}

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
    engine: &mut ApplicationEngine,
    from: &UInt160,
    to: &UInt160,
    amount: &BigInt,
    data: &[u8],
) -> CoreResult<()> {
    let gas_hash = *GAS_HASH;
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
    engine: &mut ApplicationEngine,
    account: &UInt160,
    amount: &BigInt,
    call_on_payment: bool,
) -> CoreResult<()> {
    if amount < &BigInt::zero() {
        return Err(CoreError::invalid_operation("GasToken::mint: amount cannot be negative"));
    }
    if amount.is_zero() {
        return Ok(());
    }
    let snapshot = engine.snapshot_cache();
    let balance = read_gas_account(&snapshot, account)?.unwrap_or_else(BigInt::zero) + amount;
    write_gas_account(&snapshot, account, &balance)?;
    let supply_key = StorageKey::new(GasToken::ID, vec![crate::NEP17_PREFIX_TOTAL_SUPPLY]);
    let supply = snapshot
        .get(&supply_key)
        .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
        .unwrap_or_else(BigInt::zero)
        + amount;
    snapshot.update(supply_key, StorageItem::from_bytes(crate::bigint_to_storage_bytes(&supply)));
    post_mint(engine, account, amount, call_on_payment)
}

/// C# `PostTransferAsync(null, account, amount, …)` for the mint case: emit
/// `Transfer(null, account, amount)`, then queue `onNEP17Payment(null, amount,
/// null)` when `call_on_payment` and the recipient is a deployed contract.
fn post_mint(
    engine: &mut ApplicationEngine,
    account: &UInt160,
    amount: &BigInt,
    call_on_payment: bool,
) -> CoreResult<()> {
    let gas_hash = *GAS_HASH;
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
        vec![StackItem::null(), StackItem::from_int(amount.clone()), StackItem::null()],
    );
    Ok(())
}

/// Lazily-initialised script-hash handle for the GAS native contract.
pub static GAS_HASH: LazyLock<UInt160> = LazyLock::new(|| *GAS_TOKEN_HASH);

/// The GasToken native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct GasToken;

impl GasToken {
    /// Stable native contract id (matches C# `GasToken`).
    pub const ID: i32 = -6;
    /// NEP-17 symbol (C# `GasToken.Symbol => "GAS"`).
    pub const SYMBOL: &'static str = "GAS";
    /// NEP-17 decimals (C# `GasToken.Decimals => 8`).
    pub const DECIMALS: u8 = 8;

    /// Construct a new `GasToken` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the GAS script hash.
    pub fn script_hash() -> UInt160 {
        *GAS_HASH
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
        let from_balance = read_gas_account(&snapshot, from)?;
        let to_balance = read_gas_account(&snapshot, to)?;
        match compute_gas_transfer(from_balance, to_balance, from == to, amount) {
            GasTransferOutcome::InsufficientBalance => return Ok(false),
            GasTransferOutcome::NoMovement => {}
            GasTransferOutcome::Move {
                from_new,
                delete_from,
                to_new,
            } => {
                if delete_from {
                    delete_gas_account(&snapshot, from);
                } else {
                    write_gas_account(&snapshot, from, &from_new)?;
                }
                write_gas_account(&snapshot, to, &to_new)?;
            }
        }

        // PostTransfer: emit Transfer + (if `to` is a contract) onNEP17Payment.
        post_transfer(engine, from, to, amount, data)?;
        Ok(true)
    }
}

static GAS_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        NativeMethod::new("symbol".into(), 0, true, 0, vec![], ContractParameterType::String),
        NativeMethod::new("decimals".into(), 0, true, 0, vec![], ContractParameterType::Integer),
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
        ),
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
        .with_storage_fee(50),
    ]
});

impl NativeContract for GasToken {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        *GAS_HASH
    }

    fn name(&self) -> &str {
        "GasToken"
    }

    fn methods(&self) -> &[NativeMethod] {
        &GAS_METHODS
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
                let total =
                    crate::read_storage_int(&snapshot, Self::ID, crate::NEP17_PREFIX_TOTAL_SUPPLY, 0)?;
                Ok(BigInt::from(total).to_signed_bytes_le())
            }
            "balanceOf" => {
                let account_bytes = args.first().ok_or_else(|| {
                    CoreError::invalid_operation("GasToken::balanceOf requires an account")
                })?;
                let account = UInt160::from_bytes(account_bytes).map_err(|e| {
                    CoreError::invalid_operation(format!("GasToken::balanceOf: bad account: {e}"))
                })?;
                let snapshot = engine.snapshot_cache();
                Ok(crate::read_nep17_balance(&snapshot, Self::ID, &account)?.to_signed_bytes_le())
            }
            "transfer" => {
                // C# FungibleToken.Transfer(from, to, amount, data).
                let from = UInt160::from_bytes(args.first().ok_or_else(|| {
                    CoreError::invalid_operation("GasToken::transfer requires a from account")
                })?)
                .map_err(|e| {
                    CoreError::invalid_operation(format!("GasToken::transfer: bad from: {e}"))
                })?;
                let to = UInt160::from_bytes(args.get(1).ok_or_else(|| {
                    CoreError::invalid_operation("GasToken::transfer requires a to account")
                })?)
                .map_err(|e| {
                    CoreError::invalid_operation(format!("GasToken::transfer: bad to: {e}"))
                })?;
                let amount = BigInt::from_signed_bytes_le(args.get(2).ok_or_else(|| {
                    CoreError::invalid_operation("GasToken::transfer requires an amount")
                })?);
                let data = args.get(3).map(Vec::as_slice).unwrap_or(&[]);
                // The witness bypass uses the engine's calling script hash
                // (C# `from.Equals(CallingScriptHash)`).
                let caller = engine.get_calling_script_hash().unwrap_or_else(UInt160::zero);
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
        assert_eq!(NativeContract::id(&c), -6);
        assert_eq!(NativeContract::name(&c), "GasToken");
        assert_eq!(NativeContract::hash(&c), *GAS_TOKEN_HASH);
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["symbol", "decimals", "totalSupply", "balanceOf", "transfer"]);
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
            compute_gas_transfer(None, None, false, &BigInt::zero()),
            GasTransferOutcome::NoMovement
        );

        // amount > 0, from has no account -> insufficient.
        assert_eq!(
            compute_gas_transfer(None, None, false, &amt),
            GasTransferOutcome::InsufficientBalance
        );
        // amount > 0, from underfunded -> insufficient.
        assert_eq!(
            compute_gas_transfer(Some(BigInt::from(99)), None, false, &amt),
            GasTransferOutcome::InsufficientBalance
        );
        // from == to with sufficient balance -> no movement.
        assert_eq!(
            compute_gas_transfer(Some(BigInt::from(100)), Some(BigInt::from(100)), true, &amt),
            GasTransferOutcome::NoMovement
        );
        // Exact balance -> deduct to zero deletes the from-entry; to credited.
        assert_eq!(
            compute_gas_transfer(Some(BigInt::from(100)), None, false, &amt),
            GasTransferOutcome::Move {
                from_new: BigInt::zero(),
                delete_from: true,
                to_new: BigInt::from(100)
            }
        );
        // Partial balance -> from keeps the remainder; existing to is added to.
        assert_eq!(
            compute_gas_transfer(Some(BigInt::from(250)), Some(BigInt::from(7)), false, &amt),
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

        assert!(read_gas_account(&cache, &account).unwrap().is_none());
        write_gas_account(&cache, &account, &BigInt::from(12345)).unwrap();
        assert_eq!(
            read_gas_account(&cache, &account).unwrap(),
            Some(BigInt::from(12345))
        );
        // The single-field Struct[Balance] layout is what read_nep17_balance reads.
        assert_eq!(
            crate::read_nep17_balance(&cache, GasToken::ID, &account).unwrap(),
            BigInt::from(12345)
        );
        delete_gas_account(&cache, &account);
        assert!(read_gas_account(&cache, &account).unwrap().is_none());
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
}
