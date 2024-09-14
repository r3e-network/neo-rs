use std::sync::Arc;
use std::collections::HashSet;
use NeoRust::contract::ContractManagement;
use num_bigint::BigInt;
use neo_proc_macros::contract_method;
use neo_vm::vm_types::stack_item::StackItem;
use crate::neo_contract::native_contract::{NativeContract};
use crate::hardfork::Hardfork;
use crate::neo_contract::application_engine::ApplicationEngine;
use crate::neo_contract::contract_state::ContractState;
use crate::neo_contract::key_builder::KeyBuilder;
use crate::protocol_settings::ProtocolSettings;
use crate::uint160::UInt160;
use crate::neo_contract::native_contract::contract_method_metadata::ContractMethodMetadata;
use crate::neo_contract::native_contract::contract_event_attribute::ContractEventAttribute;
use crate::neo_contract::manifest::contract_manifest::ContractManifest;
use crate::neo_contract::native_contract::account_state::AccountStateTrait;
use crate::neo_contract::storage_key::StorageKey;
use crate::neo_contract::storage_item::StorageItem;
use crate::persistence::DataCache;

pub trait FungibleToken: NativeContract {
    type State: AccountStateTrait;

    const PREFIX_TOTAL_SUPPLY: u8 = 11;
    const PREFIX_ACCOUNT: u8 = 20;

    fn factor(&self) -> &BigInt;

    #[contract_method]
    fn symbol(&self) -> String;

    #[contract_method]
    fn decimals(&self) -> u8;

    #[contract_method]
    async fn mint(&self, engine: &mut ApplicationEngine, account: &UInt160, amount: BigInt, call_on_payment: bool) -> Result<(), String> {
        if amount < BigInt::from(0) {
            return Err("Amount must be non-negative".into());
        }
        if amount == BigInt::from(0) {
            return Ok(());
        }
        let mut storage = engine.snapshot_cache.get_and_change(
            &self.create_storage_key(Self::PREFIX_ACCOUNT).add(account),
            || StorageItem::new(Self::State::default()),
        );
        let mut state: Self::State = storage.get_interoperable();
        self.on_balance_changing(engine, account, &mut state, &amount);
        state.set_balance(state.balance() + &amount);
        storage.set_interoperable(state);

        let mut total_supply = engine.snapshot_cache.get_and_change(
            &self.create_storage_key(Self::PREFIX_TOTAL_SUPPLY),
            || StorageItem::new(BigInt::from(0)),
        );
        total_supply.add(&amount);

        self.post_transfer(engine, None, Some(account), amount, StackItem::Null, call_on_payment).await
    }

    #[contract_method]
    async fn burn(&self, engine: &mut ApplicationEngine, account: &UInt160, amount: BigInt) -> Result<(), String> {
        if amount < BigInt::from(0) {
            return Err("Amount must be non-negative".into());
        }
        if amount == BigInt::from(0) {
            return Ok(());
        }
        let key = self.create_storage_key(Self::PREFIX_ACCOUNT).add(account);
        let mut storage = engine.snapshot_cache.get_and_change(&key);
        let mut state: Self::State = storage.get_interoperable();
        if state.balance() < amount {
            return Err("Insufficient balance".into());
        }
        self.on_balance_changing(engine, account, &mut state, &(-amount));
        if state.balance() == amount {
            engine.snapshot_cache.delete(&key);
        } else {
            state.set_balance(state.balance() - &amount);
            storage.set_interoperable(state);
        }
        let mut total_supply = engine.snapshot_cache.get_and_change(&self.create_storage_key(Self::PREFIX_TOTAL_SUPPLY));
        total_supply.add(&(-amount));

        self.post_transfer(engine, Some(account), None, amount, StackItem::Null, false).await
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    fn total_supply(&self, snapshot: &dyn DataCache) -> BigInt {
        snapshot.try_get(&self.create_storage_key(Self::PREFIX_TOTAL_SUPPLY))
            .map(|storage| storage.into())
            .unwrap_or_else(BigInt::zero)
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = CallFlags::READ_STATES)]
    fn balance_of(&self, snapshot: &dyn DataCache, account: &UInt160) -> BigInt {
        snapshot.try_get(&self.create_storage_key(Self::PREFIX_ACCOUNT).add(account))
            .map(|storage| storage.get_interoperable::<Self::State>().balance())
            .unwrap_or_else(BigInt::zero)
    }

    #[contract_method(cpu_fee = 1 << 17, storage_fee = 50, required_flags = CallFlags::STATES | CallFlags::ALLOW_CALL | CallFlags::ALLOW_NOTIFY)]
    async fn transfer(&self, engine: &mut ApplicationEngine, from: &UInt160, to: &UInt160, amount: BigInt, data: StackItem) -> Result<bool, String> {
        if amount < BigInt::from(0) {
            return Err("Amount must be non-negative".into());
        }
        if !from.equals(&engine.calling_script_hash) && !engine.check_witness_internal(from) {
            return Ok(false);
        }
        let key_from = self.create_storage_key(Self::PREFIX_ACCOUNT).add(from);
        let mut storage_from = engine.snapshot_cache.get_and_change(&key_from);
        
        if amount == BigInt::from(0) {
            if let Some(state_from) = storage_from.as_mut() {
                let mut state: Self::State = state_from.get_interoperable();
                self.on_balance_changing(engine, from, &mut state, &BigInt::from(0));
                state_from.set_interoperable(state);
            }
        } else {
            let mut state_from: Self::State = storage_from.get_interoperable();
            if state_from.balance() < amount {
                return Ok(false);
            }
            if from == to {
                self.on_balance_changing(engine, from, &mut state_from, &BigInt::from(0));
            } else {
                self.on_balance_changing(engine, from, &mut state_from, &(-amount));
                if state_from.balance() == amount {
                    engine.snapshot_cache.delete(&key_from);
                } else {
                    state_from.set_balance(state_from.balance() - &amount);
                    storage_from.set_interoperable(state_from);
                }
                let key_to = self.create_storage_key(Self::PREFIX_ACCOUNT).add(to);
                let mut storage_to = engine.snapshot_cache.get_and_change(&key_to, || StorageItem::new(Self::State::default()));
                let mut state_to: Self::State = storage_to.get_interoperable();
                self.on_balance_changing(engine, to, &mut state_to, &amount);
                state_to.set_balance(state_to.balance() + &amount);
                storage_to.set_interoperable(state_to);
            }
        }
        self.post_transfer(engine, Some(from), Some(to), amount, data, true).await?;
        Ok(true)
    }

    fn on_balance_changing(&self, _engine: &mut ApplicationEngine, _account: &UInt160, _state: &mut Self::State, _amount: &BigInt) {
        // Default implementation does nothing
    }

    async fn post_transfer(&self, engine: &mut ApplicationEngine, from: Option<&UInt160>, to: Option<&UInt160>, amount: BigInt, data: StackItem, call_on_payment: bool) -> Result<(), String> {
        // Send notification
        engine.send_notification(
            self.hash(),
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
            self.hash(),
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
