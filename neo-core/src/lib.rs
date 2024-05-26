// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;

pub mod block;
pub mod blockchain;

pub mod contract;
pub mod ledger;

pub mod merkle;
pub mod mpt;
pub mod nns;

pub mod payload;
pub mod store;
pub mod tx;
pub mod types;
pub mod wallet;

pub use neo_crypto::secp256r1::*;

// #[global_allocator]
// static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

// #[cfg(all(not(test), not(feature = "std")))]
// #[lang = "eh_personality"]
// #[no_mangle]
// pub extern fn eh_personality() {}