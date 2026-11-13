use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::evidence::{EVIDENCE_SCHEMA_VERSION, EvidenceBundle, EvidenceError, EvidenceKind};
use crate::stall::StallLevel;

pub const DIAGNOSIS_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionId {
    ObserveOnly,
    AlertOperator,
    CaptureConsensusSnapshot,
    RequestValidatorReview,
    EnterSafeMaintenance,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionPolicy {
    pub id: ActionId,
    pub minimum_level: StallLevel,
    pub required_evidence: EvidenceKind,
    pub requires_human_approval: bool,
}

pub const ACTION_CATALOG: [ActionPolicy; 5] = [
    ActionPolicy {
        id: ActionId::ObserveOnly,
        minimum_level: StallLevel::Level1,
        required_evidence: EvidenceKind::DetectorFinding,
        requires_human_approval: false,
    },
    ActionPolicy {
        id: ActionId::AlertOperator,
        minimum_level: StallLevel::Level1,
        required_evidence: EvidenceKind::DetectorFinding,
        requires_human_approval: false,
    },
    ActionPolicy {
        id: ActionId::CaptureConsensusSnapshot,
        minimum_level: StallLevel::Level1,
        required_evidence: EvidenceKind::ConsensusState,
        requires_human_approval: true,
    },
    ActionPolicy {
        id: ActionId::RequestValidatorReview,
        minimum_level: StallLevel::Level2,
        required_evidence: EvidenceKind::ValidatorState,
        requires_human_approval: true,
    },
    ActionPolicy {
        id: ActionId::EnterSafeMaintenance,
        minimum_level: StallLevel::Level3,
        required_evidence: EvidenceKind::ConsensusState,
        requires_human_approval: true,
    },
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionRecommendation {
    pub action: ActionId,
    pub rationale: String,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Diagnosis {
    pub schema_version: u16,
    pub evidence_schema_version: u16,
    pub observed_height: u32,
    pub summary: String,
    pub root_cause: String,
    pub confidence_bps: u16,
    pub evidence_refs: Vec<String>,
    pub actions: Vec<ActionRecommendation>,
}

#[derive(Debug, Clone, Copy)]
pub struct DiagnosisValidator {
    max_height_age: u32,
}

impl DiagnosisValidator {
    #[must_use]
    pub const fn new(max_height_age: u32) -> Self {
        Self { max_height_age }
    }

    pub fn parse_and_validate(
        &self,
        raw: &str,
        evidence: &EvidenceBundle,
        level: StallLevel,
        current_height: u32,
    ) -> Result<Diagnosis, DiagnosisError> {
        if raw.trim_start().starts_with("```") {
            return Err(DiagnosisError::NonJsonEnvelope);
        }
        let diagnosis: Diagnosis = serde_json::from_str(raw)
            .map_err(|error| DiagnosisError::InvalidJson(error.to_string()))?;
        self.validate(&diagnosis, evidence, level, current_height)?;
        Ok(diagnosis)
    }

    pub fn validate(
        &self,
        diagnosis: &Diagnosis,
        evidence: &EvidenceBundle,
        level: StallLevel,
        current_height: u32,
    ) -> Result<(), DiagnosisError> {
        evidence.validate()?;
        if diagnosis.schema_version != DIAGNOSIS_SCHEMA_VERSION
            || diagnosis.evidence_schema_version != EVIDENCE_SCHEMA_VERSION
        {
            return Err(DiagnosisError::UnsupportedVersion);
        }
        if diagnosis.observed_height > current_height
            || current_height.saturating_sub(diagnosis.observed_height) > self.max_height_age
        {
            return Err(DiagnosisError::StaleHeight {
                observed: diagnosis.observed_height,
                current: current_height,
            });
        }
        if diagnosis.summary.trim().is_empty()
            || diagnosis.root_cause.trim().is_empty()
            || diagnosis.summary.len() > 2_048
            || diagnosis.root_cause.len() > 2_048
            || diagnosis.confidence_bps > 10_000
        {
            return Err(DiagnosisError::InvalidContent);
        }
        if diagnosis.evidence_refs.is_empty() {
            return Err(DiagnosisError::MissingEvidence);
        }
        let referenced = unique_references(&diagnosis.evidence_refs)?;
        for reference in &referenced {
            let item = evidence
                .items
                .iter()
                .find(|item| item.id == *reference)
                .ok_or_else(|| DiagnosisError::UnknownEvidence(reference.clone()))?;
            if item.observed_height > current_height
                || current_height.saturating_sub(item.observed_height) > self.max_height_age
            {
                return Err(DiagnosisError::StaleEvidence(reference.clone()));
            }
        }

        let mut actions = BTreeSet::new();
        for recommendation in &diagnosis.actions {
            if !actions.insert(recommendation.action) {
                return Err(DiagnosisError::DuplicateAction(recommendation.action));
            }
            if recommendation.rationale.trim().is_empty() || recommendation.evidence_refs.is_empty()
            {
                return Err(DiagnosisError::InvalidContent);
            }
            let policy = ACTION_CATALOG
                .iter()
                .find(|policy| policy.id == recommendation.action)
                .ok_or(DiagnosisError::UnknownAction)?;
            if level < policy.minimum_level {
                return Err(DiagnosisError::ActionNotAllowed(recommendation.action));
            }
            let action_references = unique_references(&recommendation.evidence_refs)?;
            let mut required_kind_present = false;
            for reference in action_references {
                if !referenced.contains(&reference) {
                    return Err(DiagnosisError::UnknownEvidence(reference));
                }
                let item = evidence
                    .items
                    .iter()
                    .find(|item| item.id == reference)
                    .ok_or_else(|| DiagnosisError::UnknownEvidence(reference.clone()))?;
                required_kind_present |= item.kind == policy.required_evidence;
            }
            if !required_kind_present {
                return Err(DiagnosisError::MissingActionEvidence(recommendation.action));
            }
        }
        Ok(())
    }
}

fn unique_references(references: &[String]) -> Result<BTreeSet<String>, DiagnosisError> {
    let mut unique = BTreeSet::new();
    for reference in references {
        if reference.is_empty() || !unique.insert(reference.clone()) {
            return Err(DiagnosisError::InvalidEvidenceReferences);
        }
    }
    Ok(unique)
}

#[derive(Debug, Error)]
pub enum DiagnosisError {
    #[error("model output must be a bare JSON object")]
    NonJsonEnvelope,
    #[error("model output is not valid strict JSON: {0}")]
    InvalidJson(String),
    #[error("diagnostic schema version is unsupported")]
    UnsupportedVersion,
    #[error("diagnostic content is empty, oversized, or out of range")]
    InvalidContent,
    #[error("diagnosis must reference evidence")]
    MissingEvidence,
    #[error("invalid or duplicate evidence references")]
    InvalidEvidenceReferences,
    #[error("diagnosis references unknown evidence: {0}")]
    UnknownEvidence(String),
    #[error("diagnosis references stale evidence: {0}")]
    StaleEvidence(String),
    #[error("diagnosis height {observed} is stale or ahead of current height {current}")]
    StaleHeight { observed: u32, current: u32 },
    #[error("model selected an action outside the catalog")]
    UnknownAction,
    #[error("action is duplicated: {0:?}")]
    DuplicateAction(ActionId),
    #[error("action is not allowed for the detected stall level: {0:?}")]
    ActionNotAllowed(ActionId),
    #[error("action lacks evidence of its required kind: {0:?}")]
    MissingActionEvidence(ActionId),
    #[error(transparent)]
    Evidence(#[from] EvidenceError),
}
