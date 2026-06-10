//! NativeContract re-exports from `neo_execution`.
//!
//! Concrete native-contract implementations live in this crate
//! (NEO, GAS, Policy, …) but the abstract trait they implement is
//! defined alongside the application engine in `neo_execution`. This
//! module re-exports the trait and the [`NativeMethod`] metadata so
//! callers can `use neo_native_contracts::native_contract::*;`.

pub use neo_execution::native_contract::{
    is_active_for, NativeContract, NativeEvent, NativeMethod,
};
