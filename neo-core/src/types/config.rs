// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::{string::String, vec::Vec};
use core::fmt::Debug;

use crate::PublicKey;

/// constants
pub const CURRENT_TX_VERSION: u8 = 0;

pub const MAX_TX_SIZE: u32 = 102400;
pub const MAX_TX_ATTRIBUTES: u32 = 16;
pub const MAX_TXS_PER_BLOCK: u32 = 512;

pub const MAX_SIGNER_SUBITEMS: u32 = 16;
pub const MAX_MANIFEST_SIZE: u32 = 0xFFFF;

pub const MAX_SUBITEMS: u32 = 16;
pub const MAX_NESTING_DEPTH: u8 = 2;

/// neo seed endpoints of main-net
pub const SEED_LIST_MAIN_NET: &[&'static str] = &[
    "seed1.neo.org:10333",
    "seed2.neo.org:10333",
    "seed3.neo.org:10333",
    "seed4.neo.org:10333",
    "seed5.neo.org:10333",
];

/// neo seed endpoints of test-net
pub const SEED_LIST_TEST_NET: &[&'static str] = &[
    "seed1t5.neo.org:20333",
    "seed2t5.neo.org:20333",
    "seed3t5.neo.org:20333",
    "seed4t5.neo.org:20333",
    "seed5t5.neo.org:20333",
];

/// neo seed endpoints of dev-net
pub const SEED_LIST_DEV_NET: &[&'static str] =
    &["127.0.0.1:10234", "127.0.0.1:10235", "127.0.0.1:10236", "127.0.0.1:10237"];

pub const NEP_HEADER_1: u8 = 0x01;
pub const NEP_HEADER_2: u8 = 0x42;
pub const NEP_FLAG: u8 = 0xe0;

pub const MAX_SIGNERS: usize = 1024;
pub const DEFAULT_MAX_PENDING_BROADCASTS: u32 = 128;

pub const DEFAULT_PER_BLOCK_MILLIS: u64 = 15_000;

/// max block size: default is 256KiB
pub const DEFAULT_MAX_BLOCK_SIZE: usize = 0x40000;

/// max block sysfee in GAS
pub const DEFAULT_MAX_BLOCK_SYSFEE: u64 = 1500_0000_0000;
pub const DEFAULT_MAX_TXS_PER_BLOCK: u32 = 512;

pub const DEFAULT_VALIDATOR_NUM: u32 = 7;
pub const DEFAULT_COMMITTEE_NUM: u32 = 21;

/// address version
pub const ADDRESS_V3: u8 = 0x35;
pub const VALID_UNTIL_BLOCK_INCREMENT_BASE: u64 = 86_400_000;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u32)]
pub enum Network {
    MainNet = 0x00746e41,
    TestNet = 0x74746e41,
    DevNet = 0x4e454e, // i.e. PrivateNet
}

impl Network {
    pub const fn as_magic(self) -> u32 { self as u32 }
}

#[derive(Debug, Copy, Clone)]
pub enum HardFork {
    Aspidochelone,
    Basilisk,
    Cockatrice,
}

#[derive(Debug, Copy, Clone)]
pub struct HardForkHeight {
    pub hard_fork: HardFork,
    pub height: u32,
}

#[derive(Debug)]
pub struct ChainConfig {
    pub network: u32,
    // pub address_version: u8, // only v3 is supported now
    pub per_block_millis: u64,

    pub standby_committee: Vec<PublicKey>,
    pub nr_committee_members: u32,
    pub nr_validators: u32,
    // pub seeds: Vec<String>,

    /// see Tx.valid_until_block
    pub max_valid_until_block_increment: u64,

    /// Max Transactions Per Block
    pub max_txs_per_block: u32,

    /// max block sysfee in GAS
    pub max_block_sysfee: u64,

    /// i.e. MemoryPoolMaxTransactions
    pub max_txpool_size: u32,

    pub max_traceable_blocks: u32,

    /// The hard fork and the block height from which a hard fork is activated.
    pub hard_forks: Vec<HardForkHeight>,

    /// i.e. initial_gas_distribution
    pub initial_gas: u64,
}

impl Default for ChainConfig {
    fn default() -> Self {
        let increment = max_block_timestamp_increment(DEFAULT_PER_BLOCK_MILLIS);
        Self {
            network: 0,
            // address_version: ADDRESS_V3,
            per_block_millis: DEFAULT_PER_BLOCK_MILLIS,
            standby_committee: Vec::new(),
            nr_committee_members: DEFAULT_COMMITTEE_NUM,
            nr_validators: DEFAULT_VALIDATOR_NUM,
            // seeds: Vec::new(),
            max_valid_until_block_increment: increment,
            max_txs_per_block: DEFAULT_MAX_TXS_PER_BLOCK,
            max_block_sysfee: DEFAULT_MAX_BLOCK_SYSFEE,
            max_txpool_size: 50_000,
            max_traceable_blocks: 2_102_400,
            hard_forks: Vec::new(),
            initial_gas: 52_000_000_0000_0000, // 52_000_000 GAS
        }
    }
}

impl ChainConfig {
    #[inline]
    pub fn standby_validators(&self) -> &[PublicKey] {
        let take = core::cmp::min(self.nr_validators as usize, self.standby_committee.len());
        &self.standby_committee[..take]
    }

    #[inline]
    pub fn is_traceable_block(&self, current_block: u32, tx_block: u32) -> bool {
        tx_block <= current_block && tx_block + self.max_traceable_blocks > current_block
    }
}

#[inline]
pub const fn max_block_timestamp_increment(per_block_millis: u64) -> u64 {
    VALID_UNTIL_BLOCK_INCREMENT_BASE / per_block_millis
}

#[derive(Debug)]
pub struct WalletConfig {
    pub path: String,
    pub password: String,
}

#[derive(Debug)]
pub struct ConsensusConfig {
    pub enabled: bool,
    pub unlock_wallet: WalletConfig,
}

#[derive(Debug)]
pub struct AppConfig {
    pub log_level: String,
    pub log_path: String,
}
