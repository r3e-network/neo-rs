// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::encoding::{bin::ToBinEncoded, hex::ToHex};
use neo_base::errors;
use neo_base::math::U256;

use crate::{store::{self, *}, types::H160};

pub const NEO_CONTRACT_ID: u32 = 0xffff_fffb; // -5

pub const POLICY_CONTRACT_ID: u32 = 0xfffffff9; // -7

const KEY_TOTAL_SUPPLY: u8 = 11;
const PREFIX_ACCOUNT: u8 = 20;

#[derive(Debug, Copy, Clone, errors::Error)]
pub enum TransferError {
    #[error("transfer: insufficient balance of '{0}' with '{1}'")]
    InsufficientBalance(H160, U256),
}

pub struct Nep17Store<Store: store::Store> {
    contract_id: u32,
    store: Store,
}

impl<Store: store::Store> Nep17Store<Store> {
    pub fn new(contract_id: u32, store: Store) -> Self { Self { contract_id, store } }

    /// `mint` initializes token supply, for native contract
    pub(crate) fn mint(&self, account: &H160, amount: &U256) {
        if amount.is_zero() {
            return;
        }

        let key = StoreKey::new(self.contract_id, PREFIX_ACCOUNT, account);
        self.set_balance(&key, |_balance| amount.clone());
    }

    pub fn total_supply(&self) -> U256 {
        let key = StoreKey::new(self.contract_id, KEY_TOTAL_SUPPLY, &()).to_bin_encoded();
        let got = self.store.get_bin_encoded::<U256>(&key);
        match got {
            Ok(total) => total,
            Err(BinReadError::NoSuchKey) => U256::default(),
            Err(err) => core::panic!("store: Nep17Store.total_supply() got {:?}", err),
        }
    }

    pub fn balance_of(&self, account: &H160) -> U256 {
        let key = StoreKey::new(self.contract_id, PREFIX_ACCOUNT, account);
        self.balance(&key.to_bin_encoded())
    }

    // pub fn transfer(&self, key: &[u8], to: &H160, amount: &U256) -> Result<(), TransferError> {
    //     Ok(())
    // }

    fn set_balance(&self, key: &StoreKey<H160>, action: impl FnOnce(&U256) -> U256) -> bool {
        let key = key.to_bin_encoded();
        let prev = self.balance(&key);
        let curr = action(&prev);
        if prev == curr {
            // no-op
            return true;
        }

        let mut batch = self.store.write_batch();
        if curr.is_zero() {
            // TODO: with version
            batch.add_delete(key, &WriteOptions::with_always());
        } else {
            batch.add_put(key, curr.to_bin_encoded(), &WriteOptions::with_always());
        }

        let total = self.total_supply(); // TODO: with version
        batch.add_put(
            StoreKey::new(self.contract_id, KEY_TOTAL_SUPPLY, &()).to_bin_encoded(),
            (total + curr - prev).to_bin_encoded(),
            &WriteOptions::with_always(),
        );

        batch.commit().is_ok()
    }

    fn balance(&self, key: &[u8]) -> U256 {
        match self.store.get_bin_encoded::<U256>(&key) {
            Ok(balance) => balance,
            Err(BinReadError::NoSuchKey) => U256::default(),
            Err(err) => core::panic!("store: Nep17Store.balance({}) got {:?}", &key.to_hex(), err),
        }
    }
}
