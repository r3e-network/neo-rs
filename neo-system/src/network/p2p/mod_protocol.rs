//! Module registration manifest for the ported P2P protocol layer (task 89 wiring).
//!
//! This file is **not** a module of its own — it is a crate-root *declaration
//! manifest*. Task 89 splices it into the crate root of `lib.rs` with a single
//! textual include:
//!
//! ```ignore
//! // in neo-p2p/src/lib.rs, alongside the other `pub mod` declarations:
//! include!("mod_protocol.rs");
//! ```
//!
//! Because `include!` pastes these tokens as if they were written directly in
//! `lib.rs`, each `pub mod name;` below resolves to the sibling file
//! `src/name.rs` and registers the module at the **crate root**. That root-level
//! placement is exactly what the ported modules already assume: they cross-
//! reference one another through `crate::protocol_message`, `crate::ports`,
//! `crate::session_state`, and so on. Declaring this file as a normal
//! `pub mod mod_protocol;` would instead nest everything under
//! `crate::mod_protocol::*` and break those paths, so the `include!` form is
//! required.
//!
//! # Companion registrations task 89 must also add
//!
//! The modules below depend on two neighbouring module trees that are registered
//! by their own tasks and are deliberately **not** declared here (to avoid a
//! double declaration):
//!
//! * `pub mod ports;` — the typed port boundary (task 105). `src/ports/mod.rs`
//!   already exists, so a plain root declaration suffices.
//! * `pub mod sessions;` — the task-manager session bookkeeping (task 103). Task
//!   89 must add `src/sessions/mod.rs` containing `pub mod task_session;` so that
//!   `crate::sessions::task_session::TaskSession` (used by [`completion_flow`])
//!   resolves.
//!
//! # Manifest additions task 89 must also make
//!
//! * `payloads/mod.rs` must declare `pub mod headers_payload;` and
//!   `pub mod merkle_block_payload;` (the two payload types this lane added);
//!   they are referenced as `crate::payloads::headers_payload` /
//!   `crate::payloads::merkle_block_payload`.
//! * `Cargo.toml` must add the `neo-protocol` dependency named by these modules
//!   (`neo_protocol::payloads::{Block, Header, Transaction, Extensible}`).
//!
//! Nothing in this manifest compiles into the crate until task 89 performs the
//! wiring above; this lane is destination-only.

// ---------------------------------------------------------------------------
// Wire vocabulary: framing, commands, and the strongly-typed message enum.
// ---------------------------------------------------------------------------
/// Strongly-typed protocol message enum, codecs, and network framing wrapper.
pub mod protocol_message;

// ---------------------------------------------------------------------------
// Per-session dedup, filtering, and outbound bookkeeping.
// ---------------------------------------------------------------------------
/// FIFO hash dedup with TTL pruning for recently-announced inventory.
pub mod pending_known_hashes;
/// Command bit-set and size-estimated outbound queue state.
pub mod outbound_queue;
/// Bloom-filter state with a fixed-window rate limiter.
pub mod bloom_filter;
/// Handshake gate decision logic.
pub mod handshake;
/// Aggregate per-session dedup/filter/command state.
pub mod session_state;

// ---------------------------------------------------------------------------
// Task-manager sync scheduling, validation, and completion flow.
// ---------------------------------------------------------------------------
/// Task counters and request-planning functions for the sync window.
pub mod scheduling;
/// Incoming-block validation and hash-matching helpers.
pub mod block_validation;
/// Inventory-completion and persistence-event flow via typed ports.
pub mod completion_flow;

// ---------------------------------------------------------------------------
// Port-driven inbound handlers.
// ---------------------------------------------------------------------------
/// Inventory handlers (inv/getdata/getblocks/mempool/block/tx/extensible).
pub mod inventory;
/// Lightweight peer protocol handlers (ping/pong/addr/get-headers/headers).
pub mod peer_messages;

// ---------------------------------------------------------------------------
// Deprecated-notification classification.
// ---------------------------------------------------------------------------
/// Classification/summarization of `alert` and `reject` payloads.
pub mod alerts;

// ---------------------------------------------------------------------------
// Convenience re-exports of the primary public API surface.
// ---------------------------------------------------------------------------
pub use alerts::{AlertDisposition, classify_alert, summarize_reject};
pub use block_validation::{
    BlockHashMatch, IncomingBlockDisconnect, IncomingBlockOutcome, PersistedBlockMatch,
    block_matches_hash, match_persisted_block, persisted_block_hash, validate_incoming_block,
};
pub use bloom_filter::{BloomFilterState, FilterAddOutcome, FilterLoadOutcome};
pub use completion_flow::TaskManagerState;
pub use handshake::{HandshakeGateDecision, handshake_gate_decision};
pub use outbound_queue::{CommandBitSet, OutboundQueues};
pub use pending_known_hashes::PendingKnownHashes;
pub use protocol_message::{MessageHeader, NetworkMessage, ProtocolMessage};
pub use scheduling::{
    AvailableInventoryPlan, BlockIndexRequestPlan, HeaderRequestPlan, TaskCounter,
    plan_available_inventory_tasks, plan_block_index_request, plan_header_request,
};
pub use session_state::SessionState;
