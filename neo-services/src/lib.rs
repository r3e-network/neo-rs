//! # neo-services
//!
//! Typed service interfaces for Neo subsystems.
//!
//! This crate provides the **simple, foundation-level** service traits that
//! the rest of the workspace can depend on without taking a hard dependency
//! on the full `neo-core` runtime:
//!
//! - [`LedgerService`] — read-only blockchain state access (height, hash).
//! - [`StateStoreService`] — state root indices for health checks.
//! - [`MempoolService`] — pool statistics.
//! - [`PeerManagerService`] — peer connection statistics.
//! - [`RpcService`] — RPC readiness contract.
//!
//! ## Layering
//!
//! Sits in **Layer 1 (utility)**. Takes no `neo-*` dependencies: the
//! trait signatures use only primitive Rust types (`u32`, `[u8; 32]`),
//! so any layer can depend on these contracts without pulling in
//! protocol or runtime crates.
//!
//! The stateful `SystemContext` trait (which needs `StoreCache`,
//! `ApplicationEngine`, `ActorSystemHandle`, etc.) remains in
//! `neo-core::services::traits` because it is intrinsically coupled to
//! the runtime surface that lives there. This split keeps the simple
//! contracts available to any layer (e.g. RPC, telemetry, plugin
//! shims) without dragging the full runtime in.
//!
//! ## What does **not** belong here
//!
//! - Anything that needs `DataCache`, `ApplicationEngine`,
//!   `ActorSystemHandle`, `BlockchainHandle`, etc. — that lives in
//!   `neo-core::services::traits` (the `SystemContext` trait).
//! - Concrete service implementations — those live in the
//!   subsystems that own the state (e.g. `LedgerContext` impls
//!   `LedgerService` in `neo-core`).

#![doc(html_root_url = "https://docs.rs/neo-services/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod traits;

pub use traits::{
    LedgerService, MempoolService, PeerManagerService, RpcService, StateStoreService,
};
