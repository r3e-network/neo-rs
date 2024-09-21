// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

use neo_base::encoding::bin::*;

use crate::PublicKey;

pub const NEO_TOTAL_SUPPLY: u64 = 1000_000_000; // 0.1 Billion

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Role {
    Primary,
    Backup,
    WatchOnly,
}

#[derive(Debug, Clone, BinDecode, BinEncode)]
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
