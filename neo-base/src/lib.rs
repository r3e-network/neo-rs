// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(feature = "std", feature = "enclave"))]
compile_error!("feature 'std' and 'enclave' cannot be enabled both");

extern crate alloc;
extern crate core;

pub mod bytes;
pub mod encoding;
pub mod hash;
pub mod math;
pub mod merkle;

#[cfg(feature = "std")]
pub mod time;
