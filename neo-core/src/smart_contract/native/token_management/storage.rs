use super::{
    AccountState, NFTState, TokenManagement, TokenState, ID, PREFIX_ACCOUNT_STATE,
    PREFIX_TOKEN_STATE,
};
use crate::error::{CoreError, CoreResult};
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::storage_context::StorageContext;
use crate::smart_contract::StorageKey;
use crate::UInt160;
use neo_vm_rs::ExecutionEngineLimits;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use num_traits::{Signed, Zero};

impl TokenManagement {
    pub(super) fn deserialize_storage_stack_value(data: &[u8]) -> Result<StackValue, String> {
        let limits = ExecutionEngineLimits::default();
        BinarySerializer::deserialize_stack_value_with_limits(
            data,
            limits.max_item_size as usize,
            limits.max_stack_size as usize,
        )
    }

    pub(super) fn serialize_storage_stack_value(value: &StackValue) -> Result<Vec<u8>, String> {
        BinarySerializer::serialize_stack_value(value, &ExecutionEngineLimits::default())
    }

    pub(super) fn get_token_state(
        &self,
        engine: &ApplicationEngine,
        asset_id: &UInt160,
    ) -> CoreResult<Option<TokenState>> {
        let snapshot = engine.snapshot_cache();
        let key = StorageKey::create_with_uint160(ID, PREFIX_TOKEN_STATE, asset_id);
        let Some(item) = snapshot.as_ref().try_get(&key) else {
            return Ok(None);
        };
        let bytes = item.value_bytes();
        if bytes.is_empty() {
            return Ok(None);
        }
        let stack_value =
            Self::deserialize_storage_stack_value(&bytes).map_err(CoreError::native_contract)?;
        let mut token_state = TokenState::default();
        token_state.from_stack_value(stack_value)?;
        Ok(Some(token_state))
    }

    pub(super) fn get_account_state(
        &self,
        engine: &ApplicationEngine,
        asset_id: &UInt160,
        account: &UInt160,
    ) -> CoreResult<Option<AccountState>> {
        let snapshot = engine.snapshot_cache();
        let key = [
            vec![PREFIX_ACCOUNT_STATE],
            account.to_bytes().to_vec(),
            asset_id.to_bytes().to_vec(),
        ]
        .concat();
        let key = StorageKey::new(ID, key);
        let Some(item) = snapshot.as_ref().try_get(&key) else {
            return Ok(None);
        };
        let bytes = item.value_bytes();
        if bytes.is_empty() {
            return Ok(None);
        }
        let stack_value =
            Self::deserialize_storage_stack_value(&bytes).map_err(CoreError::native_contract)?;
        let mut account_state = AccountState::default();
        account_state.from_stack_value(stack_value)?;
        Ok(Some(account_state))
    }

    pub(super) fn write_account_state(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        asset_id: &UInt160,
        state: &AccountState,
    ) -> CoreResult<()> {
        let key = [
            vec![PREFIX_ACCOUNT_STATE],
            account.to_bytes().to_vec(),
            asset_id.to_bytes().to_vec(),
        ]
        .concat();
        let key = StorageKey::new(ID, key);
        if state.balance.is_zero() {
            engine.delete_storage_item(context, key.suffix())?;
        } else {
            let bytes = Self::serialize_storage_stack_value(&state.to_stack_value())
                .map_err(CoreError::native_contract)?;
            engine.put_storage_item(context, key.suffix(), &bytes)?;
        }
        Ok(())
    }

    pub(super) fn update_account_balance(
        &self,
        context: &StorageContext,
        engine: &mut ApplicationEngine,
        account: &UInt160,
        asset_id: &UInt160,
        delta: i32,
    ) -> CoreResult<()> {
        let account_key = [
            vec![PREFIX_ACCOUNT_STATE],
            account.to_bytes().to_vec(),
            asset_id.to_bytes().to_vec(),
        ]
        .concat();

        let mut balance = BigInt::from(0);
        if let Some(account_data) = engine.get_storage_item(context, &account_key) {
            if let Some(state) = Self::deserialize_account_state(&account_data) {
                balance = state.balance;
            }
        }

        balance = balance.clone() + delta;
        if balance.is_zero() {
            engine.delete_storage_item(context, &account_key)?;
        } else if balance.is_negative() {
            return Err(CoreError::native_contract(
                "TokenManagement: account balance cannot be negative",
            ));
        } else {
            let account_state = AccountState::with_balance(balance);
            self.write_account_state(context, engine, account, asset_id, &account_state)?;
        }
        Ok(())
    }

    pub(super) fn deserialize_token_state(data: &[u8]) -> Option<TokenState> {
        match Self::deserialize_storage_stack_value(data) {
            Ok(stack_value) => {
                let mut state = TokenState::default();
                state.from_stack_value(stack_value).ok()?;
                Some(state)
            }
            Err(_) => None,
        }
    }

    pub(super) fn deserialize_account_state(data: &[u8]) -> Option<AccountState> {
        match Self::deserialize_storage_stack_value(data) {
            Ok(stack_value) => {
                let mut state = AccountState::default();
                state.from_stack_value(stack_value).ok()?;
                Some(state)
            }
            Err(_) => None,
        }
    }

    pub(super) fn deserialize_nft_state(data: &[u8]) -> Option<NFTState> {
        match Self::deserialize_storage_stack_value(data) {
            Ok(stack_value) => {
                let mut state = NFTState::default();
                state.from_stack_value(stack_value).ok()?;
                Some(state)
            }
            Err(_) => None,
        }
    }
}
