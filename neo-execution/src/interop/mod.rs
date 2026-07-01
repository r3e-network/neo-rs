//! # neo-execution::interop
//!
//! Interop host glue between NeoVM execution and native/runtime services.
//!
//! ## Boundary
//!
//! This module belongs to `neo-execution`. This execution crate owns VM/native
//! interop behavior and must not own durable storage engines, P2P sync, or
//! application startup.
//!
//! ## Contents
//!
//! - `application_engine_contract`: contract syscall interop handlers.
//! - `application_engine_crypto`: crypto syscall interop handlers.
//! - `application_engine_helper`: ApplicationEngine helper syscall handlers.
//! - `application_engine_iterator`: iterator syscall interop handlers.
//! - `application_engine_op_code_prices`: application engine op code prices
//!   table and fee helpers.
//! - `application_engine_runtime`: runtime syscall interop handlers.
//! - `application_engine_storage`: storage syscall interop handlers.

pub mod application_engine_contract;
pub mod application_engine_crypto;
pub mod application_engine_helper;
pub mod application_engine_iterator;
pub mod application_engine_op_code_prices;
pub mod application_engine_runtime;
pub mod application_engine_storage;
