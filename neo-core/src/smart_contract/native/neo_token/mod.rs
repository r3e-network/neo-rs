//
// neo_token/mod.rs - NEO native token contract
//
// This module implements the NEO native token contract, which is the governance
// token of the Neo N3 blockchain. It handles:
// - NEP-17 fungible token standard (transfer, balanceOf, etc.)
// - Validator candidate registration and voting
// - Committee management and elections
// - GAS reward distribution to voters and committee members
//

use super::{
    contract_management::ContractManagement,
    fungible_token::{FungibleToken, PREFIX_ACCOUNT},
    gas_token::GasToken,
    helpers::NativeHelpers,
    native_contract::{NativeContract, NativeMethod},
    policy_contract::PolicyContract,
};
use crate::cryptography::ECPoint;
use crate::error::{CoreError, CoreResult};
use crate::hardfork::Hardfork;
use crate::persistence::{i_read_only_store::IReadOnlyStoreGeneric, seek_direction::SeekDirection};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::helper::Helper;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::ledger_contract::LedgerContract;
use crate::smart_contract::storage_context::StorageContext;
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::Contract;
use crate::smart_contract::ContractParameterType;
use crate::smart_contract::StorageItem;
use crate::UInt160;
use lazy_static::lazy_static;
use neo_vm::{stack_item::StackItem, ExecutionEngineLimits};
use num_bigint::BigInt;
use num_traits::{Signed, ToPrimitive, Zero};
use std::any::Any;

lazy_static! {
    static ref NEO_HASH: UInt160 = Helper::get_contract_hash(&UInt160::zero(), 0, "NeoToken");
}

/// NEO native token contract implementation.
///
/// This type exposes the canonical identifiers used throughout the node and
/// wires the NEP-17 surface alongside governance and committee management.
pub struct NeoToken {
    methods: Vec<NativeMethod>,
}

impl Default for NeoToken {
    fn default() -> Self {
        Self::new()
    }
}

// Constants
impl NeoToken {
    const ID: i32 = -5;
    const SYMBOL: &'static str = "NEO";
    const DECIMALS: u8 = 0;
    const NAME: &'static str = "NeoToken";
    const TOTAL_SUPPLY: i64 = 100_000_000;
    const PREFIX_VOTERS_COUNT: u8 = 1;
    const PREFIX_COMMITTEE: u8 = 14;
    const PREFIX_CANDIDATE: u8 = 33;
    const PREFIX_VOTER_REWARD_PER_COMMITTEE: u8 = 23;
    const PREFIX_GAS_PER_BLOCK: u8 = 29;
    const PREFIX_REGISTER_PRICE: u8 = 13;
    const NEO_HOLDER_REWARD_RATIO: i64 = 10;
    const COMMITTEE_REWARD_RATIO: i64 = 10;
    const VOTER_REWARD_RATIO: i64 = 80;
    const DATOSHI_FACTOR: i64 = 100_000_000;
    /// Default register price: 1000 GAS (in Datoshi)
    const DEFAULT_REGISTER_PRICE: i64 = 1000_00000000;
}

// Include implementation files
mod bonus;
mod committee;
mod governance;
mod methods;
mod native_impl;
mod nep17;
mod types;

// Re-export types for sibling modules
pub(crate) use types::{CandidateState, NeoAccountState};
