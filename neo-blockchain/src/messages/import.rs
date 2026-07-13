//! Typed requests for importing blocks into the canonical chain.

use neo_payloads::Block;
use serde::{Deserialize, Serialize};

/// Validation and durability policy for a block-import request.
///
/// The variants keep trusted local replay separate from peer sync. Both may
/// share a batch durability boundary. Trusted replay always skips observer
/// execution artifacts and live mempool maintenance; live and sync modes allow
/// artifact capture when the concrete node composition has a consumer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImportMode {
    /// Ordinary RPC, consensus, or explicit import with per-block durability.
    Live {
        /// Whether to run the canonical verified-import pipeline.
        verify: bool,
    },
    /// Peer-downloaded blocks with full verification and live side effects.
    ///
    /// The composition root may allow one durable commit for the batch when
    /// its active observers can safely defer publication until that boundary.
    Sync,
    /// Trusted local package replay, such as `chain.acc` or built-in fast sync.
    TrustedReplay {
        /// Whether to run the canonical verified-import pipeline.
        verify: bool,
    },
}

impl ImportMode {
    /// Returns whether blocks must pass the canonical verified-import pipeline.
    #[must_use]
    pub const fn verify(self) -> bool {
        match self {
            Self::Live { verify } | Self::TrustedReplay { verify } => verify,
            Self::Sync => true,
        }
    }

    /// Returns whether the input is a trusted local replay source.
    #[must_use]
    pub const fn is_trusted_replay(self) -> bool {
        matches!(self, Self::TrustedReplay { .. })
    }

    /// Returns whether native execution may retain observer replay artifacts.
    ///
    /// The composed system context makes the final per-block demand decision.
    #[must_use]
    pub const fn allows_replay_artifacts(self) -> bool {
        !self.is_trusted_replay()
    }

    /// Returns whether live mempool and cache side effects must be retained.
    #[must_use]
    pub const fn maintains_live_side_effects(self) -> bool {
        !self.is_trusted_replay()
    }
}

impl Default for ImportMode {
    fn default() -> Self {
        Self::Live { verify: true }
    }
}

/// Request to import an ordered block sequence into the canonical chain.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Import {
    /// Blocks to import.
    pub blocks: Vec<Block>,
    /// Validation, durability, and observer semantics for this request.
    pub mode: ImportMode,
}
