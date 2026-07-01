// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # neo-payloads::p2p_payloads
//!
//! P2P payload records and network inventory message types.
//!
//! ## Boundary
//!
//! This module belongs to `neo-payloads`. This protocol crate owns payload
//! records and validation helpers and must not perform IO, storage commits, or
//! service orchestration.
//!
//! ## Contents
//!
//! - `addr_payload`: P2P address payload records.
//! - `filter_add_payload`: P2P filter-add payload records.
//! - `filter_load_payload`: P2P filter-load payload records.
//! - `get_block_by_index_payload`: get block by index payload types and
//!   helpers.
//! - `get_blocks_payload`: P2P getblocks payload records.
//! - `inv_payload`: P2P inventory payload records.
//! - `network_address_with_time`: P2P address timestamp records.
//! - `node_capability`: P2P node capability records.
//! - `ping_payload`: P2P ping payload records.
//! - `version_payload`: P2P version payload records.

/// Address payload for peer discovery.
#[path = "discovery/addr_payload.rs"]
pub mod addr_payload;
/// Bloom filter add payload.
#[path = "filters/filter_add_payload.rs"]
pub mod filter_add_payload;
/// Bloom filter load payload.
#[path = "filters/filter_load_payload.rs"]
pub mod filter_load_payload;
/// Get block by index request payload.
#[path = "inventory/get_block_by_index_payload.rs"]
pub mod get_block_by_index_payload;
/// Get blocks request payload.
#[path = "inventory/get_blocks_payload.rs"]
pub mod get_blocks_payload;
/// Inventory payload for announcements.
#[path = "inventory/inv_payload.rs"]
pub mod inv_payload;
/// Network address with timestamp.
#[path = "discovery/network_address_with_time.rs"]
pub mod network_address_with_time;
/// Node capability descriptors.
#[path = "handshake/node_capability.rs"]
pub mod node_capability;
/// Ping/pong payload for keepalive.
#[path = "handshake/ping_payload.rs"]
pub mod ping_payload;
/// Version payload for handshake.
#[path = "handshake/version_payload.rs"]
pub mod version_payload;

// Re-export commonly used types
pub use addr_payload::AddrPayload;
pub use filter_add_payload::FilterAddPayload;
pub use filter_load_payload::FilterLoadPayload;
pub use get_block_by_index_payload::GetBlockByIndexPayload;
pub use get_blocks_payload::GetBlocksPayload;
pub use inv_payload::InvPayload;
pub use network_address_with_time::NetworkAddressWithTime;
pub use node_capability::NodeCapability;
pub use ping_payload::PingPayload;
pub use version_payload::VersionPayload;
