
use neo::prelude::*;
use neo::sys::{ContractMethod, ContractEvent};
use neo::vm::types::{Array, StackItem};
use neo::io::*;
use neo::persistence::*;
use neo::smart_contract::manifest::*;
use std::num::BigInt;

/// The base struct of all native tokens that are compatible with NEP-17.
pub struct FungibleToken<TState>
where
    TState: AccountState + Default,
{
    /// The symbol of the token.
    symbol: String,
    /// The number of decimal places of the token.
    decimals: u8,
    /// The factor used when calculating the displayed value of the token value.
    factor: BigInt,
}

impl<TState> NativeContract for FungibleToken<TState>
where
    TState: AccountState + Default,
{
    fn on_manifest_compose(&self, manifest: &mut ContractManifest) {
        manifest.supported_standards = vec!["NEP-17".to_string()];
    }
}

impl<TState> FungibleToken<TState>
where
    TState: AccountState + Default,
{
    const PREFIX_TOTAL_SUPPLY: u8 = 11;
    const PREFIX_ACCOUNT: u8 = 20;

    #[contract_method]
    pub fn symbol(&self) -> String {
        self.symbol.clone()
    }

    #[contract_method]
    pub fn decimals(&self) -> u8 {
        self.decimals
    }

    #[contract_event(
        name = "Transfer",
        params = [
            ("from", ContractParameterType::Hash160),
            ("to", ContractParameterType::Hash160),
            ("amount", ContractParameterType::Integer)
        ]
    )]
    pub fn new(symbol: String, decimals: u8) -> Self {
        let factor = BigInt::from(10).pow(decimals as u32);
        Self {
            symbol,
            decimals,
            factor,
        }
    }

    pub async fn mint(&self, engine: &mut ApplicationEngine, account: &Address, amount: BigInt, call_on_payment: bool) -> Result<(), String> {
        if amount < BigInt::from(0) {
            return Err("Amount must be non-negative".into());
        }
        if amount == BigInt::from(0) {
            return Ok(());
        }
        let mut storage = engine.snapshot_cache.get_and_change(
            &self.create_storage_key(Self::PREFIX_ACCOUNT).add(&account),
            || StorageItem::new(TState::default()),
        );
        let mut state: TState = storage.get_interoperable();
        self.on_balance_changing(engine, account, &mut state, &amount);
        state.balance += &amount;
        storage.set_interoperable(state);

        let mut total_supply = engine.snapshot_cache.get_and_change(
            &self.create_storage_key(Self::PREFIX_TOTAL_SUPPLY),
            || StorageItem::new(BigInt::from(0)),
        );
        total_supply.add(&amount);

        self.post_transfer(engine, None, Some(account), amount, StackItem::Null, call_on_payment).await
    }

    pub async fn burn(&self, engine: &mut ApplicationEngine, account: &Address, amount: BigInt) -> Result<(), String> {
        if amount < BigInt::from(0) {
            return Err("Amount must be non-negative".into());
        }
        if amount == BigInt::from(0) {
            return Ok(());
        }
        let key = self.create_storage_key(Self::PREFIX_ACCOUNT).add(&account);
        let mut storage = engine.snapshot_cache.get_and_change(&key);
        let mut state: TState = storage.get_interoperable();
        if state.balance < amount {
            return Err("Insufficient balance".into());
        }
        self.on_balance_changing(engine, account, &mut state, &(-amount));
        if state.balance == amount {
            engine.snapshot_cache.delete(&key);
        } else {
            state.balance -= &amount;
            storage.set_interoperable(state);
        }
        let mut total_supply = engine.snapshot_cache.get_and_change(&self.create_storage_key(Self::PREFIX_TOTAL_SUPPLY));
        total_supply.add(&(-amount));

        self.post_transfer(engine, Some(account), None, amount, StackItem::Null, false).await
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    pub fn total_supply(&self, snapshot: &DataCache) -> BigInt {
        snapshot.try_get(&self.create_storage_key(Self::PREFIX_TOTAL_SUPPLY))
            .map(|storage| storage.into())
            .unwrap_or_else(BigInt::zero)
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    pub fn balance_of(&self, snapshot: &DataCache, account: &Address) -> BigInt {
        snapshot.try_get(&self.create_storage_key(Self::PREFIX_ACCOUNT).add(&account))
            .map(|storage| storage.get_interoperable::<TState>().balance)
            .unwrap_or_else(BigInt::zero)
    }

    #[contract_method(cpu_fee = 1 << 17, storage_fee = 50, required_flags = CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY)]
    pub async fn transfer(&self, engine: &mut ApplicationEngine, from: &Address, to: &Address, amount: BigInt, data: StackItem) -> Result<bool, String> {
        if amount < BigInt::from(0) {
            return Err("Amount must be non-negative".into());
        }
        if !from.equals(&engine.calling_script_hash) && !engine.check_witness_internal(from) {
            return Ok(false);
        }
        let key_from = self.create_storage_key(Self::PREFIX_ACCOUNT).add(&from);
        let mut storage_from = engine.snapshot_cache.get_and_change(&key_from);
        
        if amount == BigInt::from(0) {
            if let Some(state_from) = storage_from.as_mut() {
                let mut state: TState = state_from.get_interoperable();
                self.on_balance_changing(engine, from, &mut state, &BigInt::from(0));
                state_from.set_interoperable(state);
            }
        } else {
            let mut state_from: TState = storage_from.get_interoperable();
            if state_from.balance < amount {
                return Ok(false);
            }
            if from == to {
                self.on_balance_changing(engine, from, &mut state_from, &BigInt::from(0));
            } else {
                self.on_balance_changing(engine, from, &mut state_from, &(-amount));
                if state_from.balance == amount {
                    engine.snapshot_cache.delete(&key_from);
                } else {
                    state_from.balance -= &amount;
                    storage_from.set_interoperable(state_from);
                }
                let key_to = self.create_storage_key(Self::PREFIX_ACCOUNT).add(&to);
                let mut storage_to = engine.snapshot_cache.get_and_change(&key_to, || StorageItem::new(TState::default()));
                let mut state_to: TState = storage_to.get_interoperable();
                self.on_balance_changing(engine, to, &mut state_to, &amount);
                state_to.balance += &amount;
                storage_to.set_interoperable(state_to);
            }
        }
        self.post_transfer(engine, Some(from), Some(to), amount, data, true).await?;
        Ok(true)
    }

    fn on_balance_changing(&self, _engine: &mut ApplicationEngine, _account: &Address, _state: &mut TState, _amount: &BigInt) {
        // Default implementation does nothing
    }

    async fn post_transfer(&self, engine: &mut ApplicationEngine, from: Option<&Address>, to: Option<&Address>, amount: BigInt, data: StackItem, call_on_payment: bool) -> Result<(), String> {
        // Send notification
        engine.send_notification(
            &self.hash(),
            "Transfer",
            Array::new(vec![
                from.map(|a| a.to_array().into()).unwrap_or(StackItem::Null),
                to.map(|a| a.to_array().into()).unwrap_or(StackItem::Null),
                amount.into(),
            ]),
        );

        // Check if it's a wallet or smart contract
        if !call_on_payment || to.is_none() || ContractManagement::get_contract(&engine.snapshot_cache, to.unwrap()).is_none() {
            return Ok(());
        }

        // Call onNEP17Payment method
        engine.call_from_native_contract(
            &self.hash(),
            to.unwrap(),
            "onNEP17Payment",
            vec![
                from.map(|a| a.to_array().into()).unwrap_or(StackItem::Null),
                amount.into(),
                data,
            ],
        ).await
    }
}
