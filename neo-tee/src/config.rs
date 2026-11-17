//! TEE/privacy configuration sections.
//!
//! Phase 3 S6c: `PrivacySection` sank from neo-node's `config::sections` so the
//! TEE runtime (`runtime::PrivacyEncryption::resolve`) can consume it without a
//! reverse dependency on the assembly layer. The node re-exports it from
//! `crate::config` to keep every historical path valid.

use serde::Deserialize;
use std::fmt;

/// Strictly opt-in encrypted-mempool privacy mode (Phase A).
///
/// With `enabled = false` (the default, or when the section is absent) the
/// node behaves exactly as before: no content-encryption engine is installed,
/// no privacy RPC endpoint is registered, and consensus keeps its existing
/// proposal path. There is no auto-enable path.
#[derive(Clone, Deserialize)]
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

/// C-7: hand-written `Debug` that never prints the committee epoch key. The
/// config section is routinely embedded in `Debug`-derived structures (e.g.
/// `NodeConfig`) and dumped to logs/diagnostics, so deriving `Debug` here
/// would leak the content-encryption secret.
impl fmt::Debug for PrivacySection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PrivacySection")
            .field("enabled", &self.enabled)
            .field("encryption_mode", &self.encryption_mode)
            .field(
                "committee_epoch_key_hex",
                &self.committee_epoch_key_hex.as_ref().map(|_| "[REDACTED]"),
            )
            .field("committee_epoch_id", &self.committee_epoch_id)
            .finish()
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn privacy_section_debug_redacts_epoch_key() {
        let secret_hex = "deadbeefcafebabe".repeat(4); // 64 hex chars
        let section = PrivacySection {
            enabled: true,
            encryption_mode: "committee_key".to_string(),
            committee_epoch_key_hex: Some(secret_hex.clone()),
            committee_epoch_id: Some(7),
        };

        let rendered = format!("{section:?}");
        assert!(rendered.contains("[REDACTED]"), "unexpected: {rendered}");
        assert!(!rendered.contains(&secret_hex), "secret leaked: {rendered}");
        // Non-secret fields stay inspectable.
        assert!(rendered.contains("committee_key"), "unexpected: {rendered}");
        assert!(rendered.contains('7'), "unexpected: {rendered}");

        let unset = format!("{:?}", PrivacySection::default());
        assert!(!unset.contains("[REDACTED]"), "unexpected: {unset}");
    }
}
