//! # neo-native-contracts::support::token
//!
//! Shared NEP token descriptors, account codecs, and storage encoding.
//!
//! ## Boundary
//!
//! This module owns byte-exact token support reused by native contracts. It
//! does not own contract-specific policy, dispatch, or lifecycle behavior.
//!
//! ## Contents
//!
//! - `nep`: NEP standards, ABI descriptors, notifications, and account state.
//! - `storage_encoding`: canonical integer storage encoding.

pub(crate) mod nep;
pub(crate) mod storage_encoding;
