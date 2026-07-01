//! # neo-wallets::assets
//!
//! Wallet asset descriptors and transfer output records.
//!
//! ## Boundary
//!
//! This module belongs to `neo-wallets`. This wallet crate owns account and
//! signing helpers and must not import blocks, run services, or mutate node
//! storage directly.
//!
//! ## Contents
//!
//! - `asset_descriptor`: wallet asset metadata records.
//! - `transfer_output`: wallet transfer output records.

/// NEP-17 asset descriptor (name / symbol / decimals lookup).
pub mod asset_descriptor;
/// NEP-17 transfer output descriptor.
pub mod transfer_output;
