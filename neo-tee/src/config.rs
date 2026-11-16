//! TEE/privacy configuration sections.
//!
//! Phase 3 S6c: `PrivacySection` sank from neo-node's `config::sections` so the
//! TEE runtime (`runtime::PrivacyEncryption::resolve`) can consume it without a
//! reverse dependency on the assembly layer. The node re-exports it from
//! `crate::config` to keep every historical path valid.

use serde::Deserialize;

/// Strictly opt-in encrypted-mempool privacy mode (Phase A).
///
/// With `enabled = false` (the default, or when the section is absent) the
/// node behaves exactly as before: no content-encryption engine is installed,
/// no privacy RPC endpoint is registered, and consensus keeps its existing
/// proposal path. There is no auto-enable path.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PrivacySection {
    /// Master switch for privacy mode. Strictly opt-in; default off.
    pub enabled: bool,
    /// Content-encryption key encapsulation mode: `"committee_key"` or
    /// `"tee_sealed_key"`.
    pub encryption_mode: String,
    /// Committee epoch key, hex-encoded (exactly 32 bytes / 64 hex chars).
    /// Required when `encryption_mode = "committee_key"` and `enabled = true`.
    pub committee_epoch_key_hex: Option<String>,
    /// Committee epoch id bound to the configured epoch key. Defaults to 1.
    pub committee_epoch_id: Option<u64>,
}

impl Default for PrivacySection {
    fn default() -> Self {
        Self {
            enabled: false,
            encryption_mode: "tee_sealed_key".to_string(),
            committee_epoch_key_hex: None,
            committee_epoch_id: None,
        }
    }
}
