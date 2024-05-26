// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::{boxed::Box, vec::Vec};
use core::cmp::Ordering;

use neo_core::{PublicKey, types::{ScriptHash, ToBftHash}};


const EFFECTIVE_VOTER_TURNOUT: u64 = 5;
const NEO_TOTAL_SUPPLY: u64 = 1000_000_000; // 0.1 Billion


#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Role {
    Primary,
    Backup,
    WatchOnly,
}


#[derive(Debug, Clone)]
pub struct Member {
    pub key: PublicKey,
    pub votes: u64, // U256,
}

pub trait MemberCache {
    /// `candidate_members` returns candidates which have registered and not be blocked.
    fn candidate_members(&self) -> Vec<Member>;

    fn committee_members(&self) -> Vec<Member>;

    fn standby_committee(&self) -> Vec<PublicKey>;

    fn voters_count(&self) -> u64;
}


#[inline]
pub fn should_refresh_committee(height: u32, nr_committee: u32) -> bool {
    nr_committee == 0 || height % nr_committee == 0
}

#[allow(dead_code)]
pub struct Committee {
    /// The number of validators, from settings, i.e. from config.
    nr_validators: u32,

    /// The number of committee, from settings.
    nr_committee: u32,

    members: Box<dyn MemberCache>,
    // cached: Vec<Member>,
}

impl Committee {
    pub fn next_block_validators(&self) -> Vec<PublicKey> {
        let mut members = self.next_committee();
        let nr_validators = self.nr_validators as usize;
        if members.len() < nr_validators {
            core::panic!("invalid the number of validators {} > {}", nr_validators, members.len());
        }

        members.truncate(nr_validators);
        members
    }

    pub fn next_committee_hash(&self) -> ScriptHash {
        self.next_committee()
            .to_bft_hash()
            .expect("to_bft_hash should be ok")
    }

    pub fn next_committee(&self) -> Vec<PublicKey> {
        let mut keys = self.members.committee_members()
            .iter()
            .map(|p| p.key.clone())
            .collect::<Vec<_>>();

        keys.sort();
        keys
    }

    pub fn compute_next_block_validators(&self) -> Vec<PublicKey> {
        let mut keys = self.compute_committee_members()
            .iter()
            .take(self.nr_validators as usize)
            .map(|member| member.key.clone())
            .collect::<Vec<_>>();

        keys.sort();
        keys
    }


    fn compute_committee_members(&self) -> Vec<Member> {
        let voters = self.members.voters_count();
        let voters = voters * EFFECTIVE_VOTER_TURNOUT;
        let voter_turnout = voters / NEO_TOTAL_SUPPLY;

        let nr_committee = self.nr_committee as usize;
        let mut candidates = self.members.candidate_members();
        let votes_of = |key: &PublicKey| {
            candidates.iter()
                .find(|candidate| candidate.key.eq(key))
                .map(|member| member.votes)
                .unwrap_or_default()
        };

        // voters_count / total_supply should be >= 0.2, select from standby if not satisfied
        if voter_turnout <= 0 || candidates.len() < nr_committee {
            return self.members.standby_committee()
                .iter()
                .take(nr_committee)
                .map(|key| Member { key: key.clone(), votes: votes_of(key) })
                .collect();
        }

        // select from candidates if satisfied
        candidates.sort_by(|lhs, rhs| {
            let ordering = lhs.votes.cmp(&rhs.votes);
            if ordering != Ordering::Equal { ordering } else { lhs.key.cmp(&rhs.key) }
        });

        candidates.iter()
            .take(nr_committee)
            .map(|member| member.clone())
            .collect()
    }
}
