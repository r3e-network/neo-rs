#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(all(feature = "std", feature = "enclave"))]
compile_error!("feature 'std' and 'enclave' cannot be enabled both");

extern crate alloc;
extern crate core;

pub mod address;
pub mod block;
pub mod h160;
pub mod h256;
pub mod io;
pub mod script;
pub mod settings;
pub mod sign;
pub mod tx;
pub mod wallet;

pub use address::{Address, ToNeo3Address, ToScriptHash, ToSignData, ADDRESS_NEO3};
pub use sign::{CheckSign, ToCheckSign, CHECK_SIGN_SIZE};
