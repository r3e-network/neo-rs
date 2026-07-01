//! # neo-wallets::scripting
//!
//! Wallet script construction and verification helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-wallets`. This wallet crate owns account and
//! signing helpers and must not import blocks, run services, or mutate node
//! storage directly.
//!
//! ## Contents
//!
//! - `scripts`: wallet script construction helpers.

pub mod scripts;
