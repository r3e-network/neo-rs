use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const EVIDENCE_SCHEMA_VERSION: u16 = 1;
const REDACTED: &str = "[REDACTED]";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    ChainProgress,
    ConsensusState,
    ValidatorState,
    PeerState,
    Timing,
    DetectorFinding,
    TransactionValidation,
    ChainCommit,
    StateRootCommit,
    SignerState,
    RecoveryState,
    ProbeResult,
    QuarantineState,
    // Keep new variants at the end to preserve existing binary enum discriminants.
    TransactionExecution,
    /// A fixed local rule finding derived from a parent-validated probe result.
    ProbeRuleFinding,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    NodeMetrics,
    ConsensusSnapshot,
    PeerSnapshot,
    DeterministicDetector,
    NodeRuntime,
    MempoolPolicy,
    RestrictedProbe,
}

/// Authority assigned by the trusted receiver after validating evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAuthority {
    /// A read-only observation that cannot authorize an action by itself.
    Observation,
    /// A repeatable deterministic rule produced this record.
    DeterministicRule,
    /// Advisory model output; never sufficient to authorize an action.
    Model,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceItem {
    pub id: String,
    pub kind: EvidenceKind,
    pub source: EvidenceSource,
    pub observed_height: u32,
    fields: BTreeMap<String, Value>,
}

impl EvidenceItem {
    pub fn new(
        id: impl Into<String>,
        kind: EvidenceKind,
        source: EvidenceSource,
        observed_height: u32,
        fields: BTreeMap<String, Value>,
    ) -> Result<Self, EvidenceError> {
        let id = id.into();
        if id.is_empty() || id.len() > 128 || !id.chars().all(valid_id_character) {
            return Err(EvidenceError::InvalidId(id));
        }
        let fields = fields
            .into_iter()
            .map(|(key, value)| {
                let sanitized = sanitize_value(Some(&key), value);
                (key, sanitized)
            })
            .collect();
        Ok(Self {
            id,
            kind,
            source,
            observed_height,
            fields,
        })
    }

    #[must_use]
    pub fn fields(&self) -> &BTreeMap<String, Value> {
        &self.fields
    }

    /// Returns the canonical SHA-256 digest of the complete sanitized item.
    pub fn content_hash(&self) -> Result<String, EvidenceError> {
        let encoded = serde_json::to_vec(self).map_err(|_| EvidenceError::Serialization)?;
        let digest = Sha256::digest(encoded);
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBundle {
    pub schema_version: u16,
    pub captured_at_unix_ms: u64,
    pub tip_height: u32,
    pub items: Vec<EvidenceItem>,
}

/// Receiver-validated, hash-addressed evidence used by action authorization.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRecord {
    pub id: String,
    pub content_hash: String,
    pub authority: EvidenceAuthority,
    pub kind: EvidenceKind,
    pub source: EvidenceSource,
    pub observed_height: u32,
    pub observed_at_unix_ms: u64,
    pub transaction_hash: Option<String>,
    pub tip_height: u32,
    pub tip_hash: Option<String>,
    pub rule_version: String,
    pub item: EvidenceItem,
}

impl EvidenceRecord {
    pub fn new(
        item: EvidenceItem,
        authority: EvidenceAuthority,
        observed_at_unix_ms: u64,
        transaction_hash: Option<String>,
        rule_version: impl Into<String>,
    ) -> Result<Self, EvidenceError> {
        let tip_height = item.observed_height;
        Self::new_with_context(
            item,
            authority,
            observed_at_unix_ms,
            transaction_hash,
            tip_height,
            None,
            rule_version,
        )
    }

    pub fn new_with_context(
        item: EvidenceItem,
        authority: EvidenceAuthority,
        observed_at_unix_ms: u64,
        transaction_hash: Option<String>,
        tip_height: u32,
        tip_hash: Option<String>,
        rule_version: impl Into<String>,
    ) -> Result<Self, EvidenceError> {
        Self::new_with_context_inner(
            item,
            authority,
            observed_at_unix_ms,
            transaction_hash,
            tip_height,
            tip_hash,
            rule_version.into(),
            false,
        )
    }

    pub(crate) fn new_validated_probe_result(
        item: EvidenceItem,
        observed_at_unix_ms: u64,
        transaction_hash: Option<String>,
        tip_height: u32,
        tip_hash: Option<String>,
        rule_version: String,
    ) -> Result<Self, EvidenceError> {
        Self::new_with_context_inner(
            item,
            EvidenceAuthority::Observation,
            observed_at_unix_ms,
            transaction_hash,
            tip_height,
            tip_hash,
            rule_version,
            true,
        )
    }

    fn new_with_context_inner(
        item: EvidenceItem,
        authority: EvidenceAuthority,
        observed_at_unix_ms: u64,
        transaction_hash: Option<String>,
        tip_height: u32,
        tip_hash: Option<String>,
        rule_version: String,
        parent_validated_probe: bool,
    ) -> Result<Self, EvidenceError> {
        let is_probe_result = item.kind == EvidenceKind::ProbeResult
            || item.source == EvidenceSource::RestrictedProbe;
        if is_probe_result
            && (item.kind != EvidenceKind::ProbeResult
                || item.source != EvidenceSource::RestrictedProbe
                || authority != EvidenceAuthority::Observation
                || !parent_validated_probe)
        {
            return Err(EvidenceError::InvalidAuthority);
        }
        if observed_at_unix_ms == 0
            || rule_version.trim().is_empty()
            || rule_version.len() > 128
            || transaction_hash
                .as_ref()
                .is_some_and(|hash| hash.is_empty() || hash.len() > 128)
            || tip_hash
                .as_ref()
                .is_some_and(|hash| hash.is_empty() || hash.len() > 128)
            || item.observed_height > tip_height.saturating_add(1)
        {
            return Err(EvidenceError::InvalidRecord);
        }
        let mut record = Self {
            id: item.id.clone(),
            content_hash: String::new(),
            authority,
            kind: item.kind,
            source: item.source,
            observed_height: item.observed_height,
            observed_at_unix_ms,
            transaction_hash,
            tip_height,
            tip_hash,
            rule_version,
            item,
        };
        record.validate_metadata()?;
        record.content_hash = record.calculate_hash()?;
        Ok(record)
    }

    pub fn verify_integrity(&self) -> Result<(), EvidenceError> {
        self.validate_metadata()?;
        self.verify_hash()
    }

    pub fn verify_hash(&self) -> Result<(), EvidenceError> {
        if self.content_hash == self.calculate_hash()? {
            Ok(())
        } else {
            Err(EvidenceError::RecordHashMismatch)
        }
    }

    fn validate_metadata(&self) -> Result<(), EvidenceError> {
        if self.id != self.item.id
            || self.kind != self.item.kind
            || self.source != self.item.source
            || self.observed_height != self.item.observed_height
            || self.observed_height > self.tip_height.saturating_add(1)
        {
            return Err(EvidenceError::RecordMetadataMismatch);
        }
        let item_transaction_hash = self
            .item
            .fields()
            .get("transaction_hash")
            .and_then(Value::as_str);
        if self.transaction_hash.as_deref() != item_transaction_hash {
            return Err(EvidenceError::TransactionBindingMismatch);
        }
        if (self.kind == EvidenceKind::ProbeResult)
            != (self.source == EvidenceSource::RestrictedProbe)
            || (self.kind == EvidenceKind::ProbeResult
                && self.authority != EvidenceAuthority::Observation)
        {
            return Err(EvidenceError::InvalidAuthority);
        }
        match self.authority {
            EvidenceAuthority::DeterministicRule => {
                let rule_matches = self
                    .item
                    .fields()
                    .get("rule_version")
                    .and_then(Value::as_str)
                    == Some(self.rule_version.as_str());
                if !matches!(
                    self.kind,
                    EvidenceKind::DetectorFinding | EvidenceKind::ProbeRuleFinding
                ) || self.source != EvidenceSource::DeterministicDetector
                    || !rule_matches
                {
                    return Err(EvidenceError::InvalidAuthority);
                }
            }
            EvidenceAuthority::Observation | EvidenceAuthority::Model => {
                if self.source == EvidenceSource::DeterministicDetector {
                    return Err(EvidenceError::InvalidAuthority);
                }
            }
        }
        Ok(())
    }

    fn calculate_hash(&self) -> Result<String, EvidenceError> {
        #[derive(Serialize)]
        struct HashMaterial<'a> {
            id: &'a str,
            authority: EvidenceAuthority,
            kind: EvidenceKind,
            source: EvidenceSource,
            observed_height: u32,
            observed_at_unix_ms: u64,
            transaction_hash: &'a Option<String>,
            tip_height: u32,
            tip_hash: &'a Option<String>,
            rule_version: &'a str,
            item: &'a EvidenceItem,
        }
        let encoded = serde_json::to_vec(&HashMaterial {
            id: &self.id,
            authority: self.authority,
            kind: self.kind,
            source: self.source,
            observed_height: self.observed_height,
            observed_at_unix_ms: self.observed_at_unix_ms,
            transaction_hash: &self.transaction_hash,
            tip_height: self.tip_height,
            tip_hash: &self.tip_hash,
            rule_version: &self.rule_version,
            item: &self.item,
        })
        .map_err(|_| EvidenceError::Serialization)?;
        let digest = Sha256::digest(encoded);
        Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
    }
}

impl EvidenceBundle {
    pub fn new(
        captured_at_unix_ms: u64,
        tip_height: u32,
        items: Vec<EvidenceItem>,
    ) -> Result<Self, EvidenceError> {
        if items.is_empty() {
            return Err(EvidenceError::EmptyBundle);
        }
        let mut ids = BTreeSet::new();
        for item in &items {
            if !ids.insert(item.id.as_str()) {
                return Err(EvidenceError::DuplicateId(item.id.clone()));
            }
            if item.observed_height > tip_height {
                return Err(EvidenceError::FutureHeight {
                    evidence_id: item.id.clone(),
                    observed: item.observed_height,
                    tip: tip_height,
                });
            }
        }
        Ok(Self {
            schema_version: EVIDENCE_SCHEMA_VERSION,
            captured_at_unix_ms,
            tip_height,
            items,
        })
    }

    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self.schema_version != EVIDENCE_SCHEMA_VERSION {
            return Err(EvidenceError::UnsupportedVersion(self.schema_version));
        }
        Self::new(
            self.captured_at_unix_ms,
            self.tip_height,
            self.items.clone(),
        )
        .map(|_| ())
    }

    #[must_use]
    pub fn contains(&self, id: &str) -> bool {
        self.items.iter().any(|item| item.id == id)
    }
}

fn valid_id_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | ':' | '.')
}

fn sanitize_value(key: Option<&str>, value: Value) -> Value {
    if key.is_some_and(is_sensitive_key) {
        return Value::String(REDACTED.to_owned());
    }
    match value {
        Value::Object(map) => Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    let value = sanitize_value(Some(&key), value);
                    (key, value)
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| sanitize_value(None, value))
                .collect(),
        ),
        other => other,
    }
}

fn is_sensitive_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase();
    [
        "api_key",
        "apikey",
        "authorization",
        "credential",
        "mnemonic",
        "password",
        "private_key",
        "secret",
        "seed",
        "signer",
        "token",
        "wallet",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum EvidenceError {
    #[error("evidence bundle must not be empty")]
    EmptyBundle,
    #[error("invalid evidence id: {0}")]
    InvalidId(String),
    #[error("duplicate evidence id: {0}")]
    DuplicateId(String),
    #[error("evidence {evidence_id} has future height {observed}, tip is {tip}")]
    FutureHeight {
        evidence_id: String,
        observed: u32,
        tip: u32,
    },
    #[error("unsupported evidence schema version: {0}")]
    UnsupportedVersion(u16),
    #[error("evidence record metadata is invalid")]
    InvalidRecord,
    #[error("evidence record serialization failed")]
    Serialization,
    #[error("evidence record content hash does not match")]
    RecordHashMismatch,
    #[error("evidence record metadata does not match its item")]
    RecordMetadataMismatch,
    #[error("evidence record transaction binding does not match its item")]
    TransactionBindingMismatch,
    #[error("evidence record authority is not valid for its source and kind")]
    InvalidAuthority,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recursively_redacts_sensitive_fields() {
        let fields = BTreeMap::from([
            ("peer_count".to_owned(), serde_json::json!(3)),
            (
                "nested".to_owned(),
                serde_json::json!({"api_key": "must-not-escape", "state": "ok"}),
            ),
        ]);
        let evidence = EvidenceItem::new(
            "peer-state",
            EvidenceKind::PeerState,
            EvidenceSource::PeerSnapshot,
            10,
            fields,
        )
        .unwrap();
        assert_eq!(evidence.fields()["nested"]["api_key"], REDACTED);
        assert_eq!(evidence.fields()["peer_count"], 3);
    }

    #[test]
    fn transaction_execution_kind_preserves_stable_serde_names() {
        assert_eq!(
            serde_json::to_string(&EvidenceKind::TransactionExecution).unwrap(),
            r#""transaction_execution""#
        );
        assert_eq!(
            serde_json::from_str::<EvidenceKind>(r#""transaction_validation""#).unwrap(),
            EvidenceKind::TransactionValidation
        );
    }

    #[test]
    fn item_hash_is_content_bound_and_nonzero() {
        let first = EvidenceItem::new(
            "item-hash",
            EvidenceKind::ConsensusState,
            EvidenceSource::ConsensusSnapshot,
            7,
            BTreeMap::from([("view".to_owned(), serde_json::json!(1))]),
        )
        .unwrap();
        let second = EvidenceItem::new(
            "item-hash",
            EvidenceKind::ConsensusState,
            EvidenceSource::ConsensusSnapshot,
            7,
            BTreeMap::from([("view".to_owned(), serde_json::json!(2))]),
        )
        .unwrap();
        assert_ne!(first.content_hash().unwrap(), "00".repeat(32));
        assert_ne!(
            first.content_hash().unwrap(),
            second.content_hash().unwrap()
        );
    }
}
