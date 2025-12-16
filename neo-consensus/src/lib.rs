//! # Neo Consensus - dBFT 2.0 Implementation
//!
//! Complete Delegated Byzantine Fault Tolerance (dBFT) consensus for the Neo blockchain.
//!
//! ## Algorithm Overview
//!
//! dBFT is a Byzantine Fault Tolerant consensus mechanism that:
//! - Achieves finality in a single block (no forks)
//! - Tolerates f = (n-1)/3 Byzantine nodes
//! - Uses a rotating speaker/validator model
//!
//! ## Core Components
//!
//! - [`ConsensusService`]: Main state machine implementing dBFT 2.0
//! - [`ConsensusContext`]: Tracks consensus state (view, validators, signatures)
//! - [`ConsensusMessageType`]: Types of consensus messages
//! - [`ChangeViewReason`]: Reasons for requesting a view change
//!
//! ## Consensus Flow
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    dBFT Consensus Flow                       │
//! │                                                              │
//! │  Speaker                    Validators                       │
//! │    │                           │                             │
//! │    │──── PrepareRequest ──────>│  (propose block)            │
//! │    │                           │                             │
//! │    │<─── PrepareResponse ──────│  (acknowledge)              │
//! │    │                           │                             │
//! │    │<──────── Commit ──────────│  (when M responses)         │
//! │    │                           │                             │
//! │    │         Block Committed   │  (when M commits)           │
//! │    ▼                           ▼                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Example
//!
//! ```rust,ignore
//! use neo_consensus::{ConsensusService, ConsensusEvent, ConsensusCommand};
//! use tokio::sync::mpsc;
//!
//! // Create event channel
//! let (event_tx, mut event_rx) = mpsc::channel(100);
//!
//! // Create consensus service
//! let mut service = ConsensusService::new(
//!     0x4E454F,           // network magic
//!     validators,          // validator list
//!     Some(0),            // my validator index
//!     private_key,        // signing key
//!     event_tx,           // event sender
//! );
//!
//! // Start consensus for block 100
//! service.start(100, timestamp)?;
//!
//! // Process incoming messages
//! service.process_message(payload)?;
//!
//! // Handle events
//! while let Some(event) = event_rx.recv().await {
//!     match event {
//!         ConsensusEvent::BlockCommitted { block_index, .. } => {
//!             println!("Block {} committed!", block_index);
//!         }
//!         ConsensusEvent::BroadcastMessage(payload) => {
//!             // Send to P2P network
//!         }
//!         _ => {}
//!     }
//! }
//! ```

pub mod change_view_reason;
pub mod context;
pub mod error;
pub mod message_type;
pub mod messages;
pub mod service;

// Re-exports - Types
pub use change_view_reason::ChangeViewReason;
pub use error::{ConsensusError, ConsensusResult};
pub use message_type::ConsensusMessageType;

// Re-exports - Context
pub use context::{ConsensusContext, ConsensusState, ValidatorInfo, BLOCK_TIME_MS, MAX_VALIDATORS};

// Re-exports - Messages
pub use messages::{
    ChangeViewMessage, CommitMessage, ConsensusMessage, ConsensusPayload, PrepareRequestMessage,
    PrepareResponseMessage, RecoveryMessage, RecoveryRequestMessage,
};

// Re-exports - Service
pub use service::{BlockData, ConsensusCommand, ConsensusEvent, ConsensusService};
