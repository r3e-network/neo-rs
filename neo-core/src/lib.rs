// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
extern crate core;

pub mod block;

#[cfg(feature = "std")]
pub mod blockchain;

pub mod contract;
pub mod ledger;

pub mod merkle;
pub mod mpt;
pub mod nns;

pub mod payload;
pub mod store;
pub mod tx;
pub mod wallet;
pub mod neo_contract;
pub mod network;
pub mod persistence;
pub mod core_error;
pub mod big_decimal;
pub mod io;
pub mod neo_system;
pub mod protocol_settings;
pub mod utility;
pub mod hardfork;
pub mod contains_transaction_type;
pub mod cryptography;
pub mod time_provider;
pub mod event;

pub use neo_crypto::secp256r1::*;

// #[global_allocator]
// static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// #[cfg(all(not(test), not(feature = "std")))]
// #[lang = "eh_personality"]
// #[no_mangle]
// pub extern fn eh_personality() {}
