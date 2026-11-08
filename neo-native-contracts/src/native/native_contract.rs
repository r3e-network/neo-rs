//! Native-side re-exports of engine-owned execution interfaces.
//!
//! Migrated from `neo-core/src/smart_contract/native/native_contract.rs`.
//!
//! The concrete trait/type definitions are owned by the engine crate
//! (`neo-smart-contract`); this leaf only re-exports them so the concrete
//! native contracts can name them through `super::` / `crate::native::`
//! without reaching back into `neo-core`.
//!
//! Source path `crate::smart_contract::application_engine::native_contract`
//! becomes `neo_smart_contract::application_engine::native_contract`. All six
//! items are public there (`BaseNativeContract`, `NativeContract`,
//! `NativeMethod`, `is_active_for` are defined in the submodule;
//! `NativeContractsCache` / `NativeContractsCacheEntry` are re-exported into it
//! by the engine module root).

pub use neo_smart_contract::application_engine::native_contract::{
    BaseNativeContract, NativeContract, NativeContractsCache, NativeContractsCacheEntry,
    NativeMethod, is_active_for,
};
