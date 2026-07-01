// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # neo-execution
//!
//! Neo application execution, interop host logic, contract state, and storage
//! helpers.
//!
//! ## Boundary
//!
//! This execution crate owns VM/native interop behavior and must not own
//! durable storage engines, P2P sync, or application startup.
//!
//! ## Contents
//!
//! - `application_engine`: ApplicationEngine interop groups and execution-
//!   facing syscall handlers.
//! - `contracts`: Contract metadata, manifests, deployed-state records, and
//!   contract parameter types.
//! - `interop`: Interop host glue between NeoVM execution and native/runtime
//!   services.
//! - `iterators`: Iterator adapters exposed to contract execution and storage
//!   search.
//! - `native`: Native contract abstractions and registries used by execution.
//! - `runtime`: Runtime flags, execution context state, and VM-facing support
//!   types.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.

// ============================================================================
// Application engine
// ============================================================================
pub mod application_engine;

/// Smart-contract model types used by the execution engine.
pub mod contracts;
/// NeoVM syscall handlers registered into the application engine.
pub mod interop;

// ============================================================================
// Iterators
// ============================================================================
pub mod iterators;

/// Native-contract traits, registries, and metadata consumed by the engine.
pub mod native;

/// Runtime support types for diagnostics, helpers, and VM interop wrappers.
pub mod runtime;
/// Storage-key and storage-context helpers used during execution.
pub mod storage;

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

pub use contracts::{
    contract, contract_parameter, contract_parameters_context, contract_state, deployed_contract,
    max_length_attribute,
};
pub use interop::{
    application_engine_contract, application_engine_crypto, application_engine_helper,
    application_engine_iterator, application_engine_op_code_prices, application_engine_runtime,
    application_engine_storage,
};
pub use native::{
    hardfork_activable, native_contract, native_contract_cache, native_contract_provider,
    native_registry,
};
pub use runtime::{
    bls12381_interop, diagnostic, engine_provider, env_flags, execution_context_state, helper,
    interoperable, notify_event_args,
};
pub use storage::{key_builder, storage_context, storage_item_ext};
