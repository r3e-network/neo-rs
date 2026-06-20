// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # Neo Execution Engine
//!
//! The canonical home for the Neo N3 smart-contract execution engine and
//! all of the interop, storage, iterator, and contract-management helpers
//! the engine needs to dispatch syscalls. The actual native contracts
//! (NEO, GAS, Policy, Oracle, …) live in the [`neo_native_contracts`]
//! crate; the seam between this crate and the native-contracts crate is
//! the abstract [`NativeContract`] trait and the [`NativeRegistry`] type
//! that the engine consults when dispatching `System.Contract.CallNative`.
//!
//! ## Layer position
//!
//! This is a **Layer 2 (Service)** crate. It depends on the foundation
//! crates (`neo-vm`, `neo-primitives`, `neo-crypto`, `neo-config`,
//! `neo-storage`, `neo-io`, `neo-error`, `neo-serialization`,
//! `neo-manifest`, `neo-payloads`) and the [`neo_native_contracts`]
//! crate provides the concrete `NativeContract` implementations.

#![allow(dead_code)]

// ============================================================================
// Application engine
// ============================================================================
pub mod application_engine;

// ============================================================================
// Interop service modules (registered into the ApplicationEngine)
// ============================================================================
pub mod application_engine_contract;
pub mod application_engine_crypto;
pub mod application_engine_helper;
pub mod application_engine_iterator;
pub mod application_engine_op_code_prices;
pub mod application_engine_runtime;
pub mod application_engine_storage;

// ============================================================================
// Iterators
// ============================================================================
pub mod iterators;

// ============================================================================
// Native contract abstract types (trait, registry, cache, method metadata)
// ============================================================================
pub mod hardfork_activable;
pub mod native_contract;
/// Cached native-contract state used while composing native manifests and storage.
pub mod native_contract_cache;
pub mod native_contract_provider;
pub mod native_registry;

// ============================================================================
// Core smart-contract data types
// ============================================================================
/// `InteropInterface` wrapper for BLS12-381 curve points (CryptoLib).
pub mod bls12381_interop;
pub mod contract;
pub mod contract_parameter;
pub mod contract_parameters_context;
pub mod contract_state;
pub mod deployed_contract;
pub mod diagnostic;
pub mod engine_provider;
/// Environment flag helpers used by execution diagnostics and optional profiling.
pub mod env_flags;
pub mod execution_context_state;
pub mod helper;
pub mod interoperable;
pub mod key_builder;
pub mod max_length_attribute;
pub mod notify_event_args;
pub mod storage_context;
pub mod storage_item_ext;

// ============================================================================
// Re-exports at the crate root
// ============================================================================
pub use application_engine::ApplicationEngine;
pub use bls12381_interop::Bls12381Interop;
pub use contract::Contract;
pub use contract_parameter::ContractParameter;
pub use contract_parameters_context::ContractParametersContext;
pub use contract_state::ContractState;
pub use deployed_contract::DeployedContract;
pub use diagnostic::{Diagnostic, InstructionCounter};
pub use engine_provider::ApplicationEngineProvider;
// `env_flag_enabled` stays crate-private to `env_flags` (it is only used inside the engine).
pub use execution_context_state::ExecutionContextState;
pub use hardfork_activable::HardforkActivable;
pub use helper::Helper;
pub use interoperable::Interoperable;
pub use key_builder::KeyBuilder;
pub use max_length_attribute::MaxLengthAttribute;
pub use native_contract::{NativeContract, NativeEvent, NativeMethod, is_active_for};
pub use native_contract_cache::{NativeContractsCache, NativeContractsCacheEntry};
pub use native_registry::NativeRegistry;
pub use neo_primitives::TriggerType;
pub use notify_event_args::NotifyEventArgs;
pub use storage_context::StorageContext;
pub use storage_item_ext::StorageItemExt;
