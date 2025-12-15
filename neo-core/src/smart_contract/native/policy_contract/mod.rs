//! Policy contract native implementation.
//!
//! Mirrors the behaviour of `Neo.SmartContract.Native.PolicyContract` from the
//! Neo N3 C# reference node.

use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::neo_config::ADDRESS_SIZE;
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::manifest::{ContractEventDescriptor, ContractParameterDefinition};
use crate::smart_contract::native::{NativeContract, NativeMethod};
use crate::smart_contract::storage_key::StorageKey;
use crate::smart_contract::{ContractParameterType, StorageItem};
use crate::UInt160;
use neo_primitives::TransactionAttributeType;
use neo_vm::StackItem;
use num_bigint::{BigInt, Sign};
use num_traits::{ToPrimitive, Zero};
use std::any::Any;

/// The Policy native contract.
pub struct PolicyContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl PolicyContract {
    const ID: i32 = -7;
    const CPU_FEE: i64 = 1 << 15;

    /// The default execution fee factor.
    pub const DEFAULT_EXEC_FEE_FACTOR: u32 = 30;

    /// The default storage price.
    pub const DEFAULT_STORAGE_PRICE: u32 = 100_000;

    /// The default network fee per byte of transactions.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS.
    pub const DEFAULT_FEE_PER_BYTE: u32 = 1_000;

    /// The default fee for attributes.
    pub const DEFAULT_ATTRIBUTE_FEE: u32 = 0;

    /// The default fee for `NotaryAssisted` attribute after Echidna.
    pub const DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE: u32 = 10_000_000;

    /// Maximum execution fee factor committee can set.
    pub const MAX_EXEC_FEE_FACTOR: u32 = 100;

    /// Maximum attribute fee committee can set.
    pub const MAX_ATTRIBUTE_FEE: u32 = 10_0000_0000;

    /// Maximum storage price committee can set.
    pub const MAX_STORAGE_PRICE: u32 = 10_000_000;

    /// Maximum block generation time committee can set in milliseconds.
    pub const MAX_MILLISECONDS_PER_BLOCK: u32 = 30_000;

    /// Maximum MaxValidUntilBlockIncrement committee can set.
    pub const MAX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u32 = 86_400;

    /// Maximum MaxTraceableBlocks committee can set.
    pub const MAX_MAX_TRACEABLE_BLOCKS: u32 = 2_102_400;

    const PREFIX_BLOCKED_ACCOUNT: u8 = 15;
    const PREFIX_FEE_PER_BYTE: u8 = 10;
    const PREFIX_EXEC_FEE_FACTOR: u8 = 18;
    const PREFIX_STORAGE_PRICE: u8 = 19;
    const PREFIX_ATTRIBUTE_FEE: u8 = 20;
    const PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
    const PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
    const PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

    const MILLISECONDS_PER_BLOCK_CHANGED_EVENT_NAME: &'static str = "MillisecondsPerBlockChanged";

    /// Creates a new Policy contract.
    pub fn new() -> Self {
        // Policy contract hash: 0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b
        let hash = UInt160::from_bytes(&[
            0xcc, 0x5e, 0x4e, 0xdd, 0x9f, 0x5f, 0x8d, 0xba, 0x8b, 0xb6, 0x57, 0x34, 0x54, 0x1d,
            0xf7, 0xa1, 0xc0, 0x81, 0xc6, 0x7b,
        ])
        .expect("PolicyContract hash should be valid");

        let methods = vec![
            NativeMethod::safe(
                "getFeePerByte".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getExecFeeFactor".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getStoragePrice".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getMillisecondsPerBlock".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getMaxValidUntilBlockIncrement".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES),
            NativeMethod::safe(
                "getMaxTraceableBlocks".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES),
            // getAttributeFee overloads (hardfork switch at Echidna).
            NativeMethod::safe(
                "getAttributeFee".to_string(),
                Self::CPU_FEE,
                vec![ContractParameterType::Integer],
                ContractParameterType::Integer,
            )
            .with_deprecated_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["attributeType".to_string()]),
            NativeMethod::safe(
                "getAttributeFee".to_string(),
                Self::CPU_FEE,
                vec![ContractParameterType::Integer],
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["attributeType".to_string()]),
            // Setters.
            NativeMethod::unsafe_method(
                "setFeePerByte".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setExecFeeFactor".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setStoragePrice".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setMillisecondsPerBlock".to_string(),
                Self::CPU_FEE,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setMaxValidUntilBlockIncrement".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["value".to_string()]),
            NativeMethod::unsafe_method(
                "setMaxTraceableBlocks".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["value".to_string()]),
            // setAttributeFee overloads (hardfork switch at Echidna).
            NativeMethod::unsafe_method(
                "setAttributeFee".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_deprecated_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["attributeType".to_string(), "value".to_string()]),
            NativeMethod::unsafe_method(
                "setAttributeFee".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(vec!["attributeType".to_string(), "value".to_string()]),
            // Account policy.
            NativeMethod::safe(
                "isBlocked".to_string(),
                Self::CPU_FEE,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_required_call_flags(CallFlags::READ_STATES)
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::unsafe_method(
                "blockAccount".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::unsafe_method(
                "unblockAccount".to_string(),
                Self::CPU_FEE,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_parameter_names(vec!["account".to_string()]),
            NativeMethod::safe(
                "getBlockedAccounts".to_string(),
                Self::CPU_FEE,
                Vec::new(),
                ContractParameterType::InteropInterface,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_required_call_flags(CallFlags::READ_STATES),
        ];

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }
}

// Include implementation files
mod helpers;
mod getters;
mod setters;
mod account;
mod snapshot;
mod native_impl;
