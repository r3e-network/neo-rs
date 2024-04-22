// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::vec::Vec;

use crate::store::{self, Version, WriteOptions};


/// neo contract id is -5
pub const NEO_CONTRACT_ID: u32 = 0xFFFF_FFFB;

pub const VOTER_REWARD_FACTOR: u64 = 100_000_000;

// const PREFIX_CANDIDATE: u32 = 33;
// const PREFIX_VOTERS_COUNT: u32 = 1;
//
// const PREFIX_GASPER_BLOCK: u32 = 29;
// const PREFIX_REGISTER_PRICE: u32 = 13;
//
// const PREFIX_VOTER_REWARD_PER_COMMITTEE: u64 = 23;


#[derive(Debug, Clone)]
pub struct DalVersion {
    pub prefix: u8,
    pub network: u32,

    // pub p2p_sign: bool,
    // pub p2p_state_exchange: bool,
    // pub keep_latest_state_only: bool,
    // pub state_root_in_header: bool,
    // pub value: String,
}


impl DalVersion {
    pub fn to_store_key(&self, id: u32, key: &[u8]) -> Vec<u8> {
        let mut buf = Vec::with_capacity(1 + 4 + key.len());
        buf.push(self.prefix);
        buf.extend_from_slice(&id.to_le_bytes());
        buf.extend_from_slice(key);

        buf
    }
}

/// Data Access Layer
pub struct DalStore<Store: store::Store> {
    version: DalVersion,
    store: Store,
}


impl<Store: store::Store> DalStore<Store> {
    pub fn new(version: DalVersion, store: Store) -> Self {
        Self { version, store }
    }

    #[inline]
    pub fn get(&self, id: u32, key: &[u8]) -> Result<(Vec<u8>, Version), Store::ReadError> {
        let store_key = self.version.to_store_key(id, key);
        self.store.get(&store_key)
    }

    #[inline]
    pub fn put(&self, id: u32, key: &[u8], value: &[u8]) -> Result<Version, Store::WriteError> {
        let store_key = self.version.to_store_key(id, key);
        self.store.put(&store_key, value, &WriteOptions::default())
    }
}
