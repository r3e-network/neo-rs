use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StallLevel {
    Level1,
    Level2,
    Level3,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownReason {
    MissingNetworkHeight,
    MissingPeerHeight,
    ClockRegression,
    InconsistentHeights,
    MissingValidatorSet,
    InconsistentValidatorSet,
    NoConnectivity,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DetectionResult {
    Healthy {
        local_height: u32,
        network_height: u32,
    },
    Stalled {
        level: StallLevel,
        local_height: u32,
        network_height: u32,
        no_progress_ms: u64,
    },
    Unknown {
        reason: UnknownReason,
    },
}

impl DetectionResult {
    #[must_use]
    pub const fn stall_level(&self) -> Option<StallLevel> {
        match self {
            Self::Stalled { level, .. } => Some(*level),
            Self::Healthy { .. } | Self::Unknown { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConsensusObservation {
    pub local_height: u32,
    pub network_height: Option<u32>,
    pub max_peer_height: Option<u32>,
    pub last_progress_unix_ms: u64,
    pub observed_at_unix_ms: u64,
    pub view_number: u32,
    pub connected_peers: u32,
    pub active_validators: u16,
    pub expected_validators: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct StallPolicy {
    pub level1_after_ms: u64,
    pub level2_after_ms: u64,
    pub level3_after_ms: u64,
}

impl Default for StallPolicy {
    fn default() -> Self {
        Self {
            level1_after_ms: 30_000,
            level2_after_ms: 90_000,
            level3_after_ms: 300_000,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StallDetector {
    policy: StallPolicy,
}

impl StallDetector {
    pub fn new(policy: StallPolicy) -> Result<Self, StallPolicyError> {
        if policy.level1_after_ms == 0
            || policy.level1_after_ms >= policy.level2_after_ms
            || policy.level2_after_ms >= policy.level3_after_ms
        {
            return Err(StallPolicyError::InvalidThresholds);
        }
        Ok(Self { policy })
    }

    #[must_use]
    pub fn detect(&self, observation: &ConsensusObservation) -> DetectionResult {
        let Some(network_height) = observation.network_height else {
            return DetectionResult::Unknown {
                reason: UnknownReason::MissingNetworkHeight,
            };
        };
        if observation.observed_at_unix_ms < observation.last_progress_unix_ms {
            return DetectionResult::Unknown {
                reason: UnknownReason::ClockRegression,
            };
        }
        if observation.connected_peers > 0 && observation.max_peer_height.is_none() {
            return DetectionResult::Unknown {
                reason: UnknownReason::MissingPeerHeight,
            };
        }
        let observed_network_height = observation
            .max_peer_height
            .map_or(observation.local_height, |height| {
                observation.local_height.max(height)
            });
        if network_height != observed_network_height {
            return DetectionResult::Unknown {
                reason: UnknownReason::InconsistentHeights,
            };
        }
        if observation.expected_validators == 0 {
            return DetectionResult::Unknown {
                reason: UnknownReason::MissingValidatorSet,
            };
        }
        if observation.active_validators > observation.expected_validators {
            return DetectionResult::Unknown {
                reason: UnknownReason::InconsistentValidatorSet,
            };
        }
        let no_progress_ms = observation
            .observed_at_unix_ms
            .saturating_sub(observation.last_progress_unix_ms);
        let level = if no_progress_ms >= self.policy.level3_after_ms {
            Some(StallLevel::Level3)
        } else if no_progress_ms >= self.policy.level2_after_ms {
            Some(StallLevel::Level2)
        } else if no_progress_ms >= self.policy.level1_after_ms {
            Some(StallLevel::Level1)
        } else {
            None
        };

        match level {
            Some(_) if observation.connected_peers == 0 => DetectionResult::Unknown {
                reason: UnknownReason::NoConnectivity,
            },
            Some(level) => DetectionResult::Stalled {
                level,
                local_height: observation.local_height,
                network_height,
                no_progress_ms,
            },
            None => DetectionResult::Healthy {
                local_height: observation.local_height,
                network_height,
            },
        }
    }
}

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum StallPolicyError {
    #[error("stall thresholds must be non-zero and strictly increasing")]
    InvalidThresholds,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(no_progress_ms: u64) -> ConsensusObservation {
        ConsensusObservation {
            local_height: 10,
            network_height: Some(11),
            max_peer_height: Some(11),
            last_progress_unix_ms: 1_000,
            observed_at_unix_ms: 1_000 + no_progress_ms,
            view_number: 2,
            connected_peers: 3,
            active_validators: 7,
            expected_validators: 7,
        }
    }

    #[test]
    fn classifies_all_three_levels_deterministically() {
        let detector = StallDetector::new(StallPolicy::default()).unwrap();
        assert_eq!(
            detector.detect(&observation(30_000)).stall_level(),
            Some(StallLevel::Level1)
        );
        assert_eq!(
            detector.detect(&observation(90_000)).stall_level(),
            Some(StallLevel::Level2)
        );
        assert_eq!(
            detector.detect(&observation(300_000)).stall_level(),
            Some(StallLevel::Level3)
        );
    }

    #[test]
    fn equal_height_with_sustained_no_progress_is_stalled() {
        let detector = StallDetector::new(StallPolicy::default()).unwrap();
        let mut value = observation(90_000);
        value.network_height = Some(value.local_height);
        value.max_peer_height = Some(value.local_height);
        assert_eq!(
            detector.detect(&value),
            DetectionResult::Stalled {
                level: StallLevel::Level2,
                local_height: 10,
                network_height: 10,
                no_progress_ms: 90_000,
            }
        );
    }

    #[test]
    fn equal_height_without_peer_evidence_is_unknown_after_threshold() {
        let detector = StallDetector::new(StallPolicy::default()).unwrap();
        let mut value = observation(30_000);
        value.network_height = Some(value.local_height);
        value.max_peer_height = None;
        value.connected_peers = 0;
        assert_eq!(
            detector.detect(&value),
            DetectionResult::Unknown {
                reason: UnknownReason::NoConnectivity
            }
        );
    }

    #[test]
    fn returns_unknown_when_required_observation_is_missing() {
        let detector = StallDetector::new(StallPolicy::default()).unwrap();
        let mut value = observation(300_000);
        value.network_height = None;
        assert_eq!(
            detector.detect(&value),
            DetectionResult::Unknown {
                reason: UnknownReason::MissingNetworkHeight
            }
        );
    }
}
