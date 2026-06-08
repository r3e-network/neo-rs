//! # neo-chain
//!
//! Pure block / chain validation for the Neo blockchain.
//!
//! This crate owns the **stateless** validation rules: size limits,
//! transaction counts, timestamp bounds, merkle-root recomputation,
//! duplicate-transaction detection, witness-script sanity checks. It
//! operates on the `BlockLike` trait from `neo-primitives` and on
//! `&Witness` references, so it has no dependency on the stateful
//! `Header` / `Transaction` / `Blockchain` / `DataCache` types that
//! live in `neo-core`.
//!
//! ## Layering
//!
//! Sits in **Layer 2 (service)**. May depend on:
//! - `neo-crypto`, `neo-error`, `neo-primitives`, `neo-time` (Layer 0).
//! - `neo-ledger-types` (Layer 1) — for `Witness`.
//!
//! Must **not** depend on `neo-core` (Layer 1/2 orchestrator),
//! `neo-storage` (Layer 1) state caches, `neo-smart-contract-types` /
//! `neo-execution` (Layer 1) native contracts, or any Layer 2+ crate
//! that needs stateful access. This is the same rule reth's
//! `reth-consensus` and polkadot-sdk's `sp-consensus` follow: keep the
//! pure rule set independent of the runtime that ultimately enforces
//! it.
//!
//! ## Stateful validation lives in `neo-core`
//!
//! Anything that needs `DataCache`, `HeaderCache`, native-contract
//! lookup, GAS accounting, or policy fee rules lives in
//! `neo-core::BlockVerificationExt` as an extension trait on the
//! block / header types. `neo-chain` is the pure predecessor; the
//! caller composes the two at the chain-orchestration edge.

#![doc(html_root_url = "https://docs.rs/neo-chain/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod block_validation;
