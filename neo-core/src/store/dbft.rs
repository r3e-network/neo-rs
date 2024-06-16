// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::encoding::bin::*;
use crate::{PublicKey, types::{Member, ToBftHash, NEO_TOTAL_SUPPLY}};
use crate::store::{self, DEFAULT_STORAGE_PRICE, NEO_CONTRACT_ID, Nep17Store, StoreKey, WriteBatch, WriteOptions};


pub const PREFIX_VOTERS_COUNT: u8 = 1;
pub const PREFIX_CANDIDATE: u8 = 33;

pub const PREFIX_REGISTER_PRICE: u8 = 13;
pub const PREFIX_COMMITTEE: u8 = 14;
pub const PREFIX_GASPER_BLOCK: u8 = 29;

pub const PREFIX_VOTER_REWARD_PER_COMMITTEE: u8 = 23;


pub struct NeoBalance {
    pub balance: u64,
    pub last_gas_per_vote: u64,
    pub block_index: u32,
    pub vote_to: PublicKey,
}


pub struct Candidate {
    pub registered: bool,
    pub votes: u64,
}


pub struct DbftStore<Store: store::Store> {
    contract_id: u32,
    nr_committee: u32,
    nr_validators: u32,
    standbys: Vec<PublicKey>,
    committee: Vec<Member>,

    contract: Nep17Store<Store>,
    store: Store,
}

impl<Store: store::Store> DbftStore<Store> {
    pub fn new(nr_committee: u32, nr_validators: u32, standbys: Vec<PublicKey>, store: Store) -> Self {
        if standbys.is_empty() || standbys.len() < nr_validators as usize {
            core::panic!("store::dbft: invalid standbys.len({}) or nr_validators({})", standbys.len(), nr_validators);
        }

        let committee = standbys.iter()
            .map(|key| Member { key: key.clone(), votes: 0 })
            .collect();
        let dbft = Self {
            contract_id: NEO_CONTRACT_ID,
            nr_committee,
            nr_validators,
            standbys,
            committee,
            contract: Nep17Store::new(NEO_CONTRACT_ID, store.clone()),
            store,
        };

        dbft.on_initial();
        dbft
    }

    fn on_initial(&self) {
        let key = StoreKey::new(self.contract_id, PREFIX_COMMITTEE, &());
        self.store.put(
            key.to_bin_encoded(),
            self.committee.to_bin_encoded(),
            &WriteOptions::with_always(),
        ).expect("`store.put` committee should be ok");

        let standbys_hash = (&self.standbys[0..self.nr_validators as usize])
            .to_bft_hash()
            .expect("`standbys.to_bft_hash` should be ok");

        // TODO: emit Transfer event
        self.contract.mint(&standbys_hash.into(), &NEO_TOTAL_SUPPLY.into());

        let mut batch = self.store.write_batch();
        batch.add_put(
            StoreKey::new(self.contract_id, PREFIX_VOTERS_COUNT, &()).to_bin_encoded(),
            0u64.to_bin_encoded(),
            &WriteOptions::with_always(),
        );

        batch.add_put(
            StoreKey::new(self.contract_id, PREFIX_REGISTER_PRICE, &()).to_bin_encoded(),
            DEFAULT_STORAGE_PRICE.to_bin_encoded(),
            &WriteOptions::with_always(),
        );

        let _ = batch.commit()
            .expect("`batch.commit()` should be ok on_initial");
    }
}