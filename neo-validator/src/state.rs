//! Atomic validation watermarks and fail-closed proxy gate state.

use crate::upstream::NodeKind;
use anyhow::{Result, bail};
use neo_primitives::UInt256;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::array;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ValidationPhase {
    NotReady,
    Verifying,
    Healthy,
    Mismatch,
    CapabilityError,
    ExecutorError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifiedPoint {
    pub height: u32,
    pub block_hash: UInt256,
    pub state_root: UInt256,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Incident {
    pub id: String,
    pub height: u32,
    pub code: String,
    pub detail: String,
    pub observed_at_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamStatus {
    pub available: bool,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorStatus {
    pub phase: ValidationPhase,
    pub verified: Option<VerifiedPoint>,
    pub incident: Option<Incident>,
    pub neo_cli: UpstreamStatus,
    pub neo_go: UpstreamStatus,
    pub generation: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct ProxyPermit {
    pub generation: u64,
    pub verified_height: u32,
}

struct Inner {
    phase: RwLock<ValidationPhase>,
    verified: RwLock<Option<VerifiedPoint>>,
    incident: RwLock<Option<Incident>>,
    upstreams: [RwLock<UpstreamStatus>; 2],
    generation: AtomicU64,
}

#[derive(Clone)]
pub struct GateRegistry {
    inner: Arc<Inner>,
}

impl Default for GateRegistry {
    fn default() -> Self {
        Self {
            inner: Arc::new(Inner {
                phase: RwLock::new(ValidationPhase::NotReady),
                verified: RwLock::new(None),
                incident: RwLock::new(None),
                upstreams: array::from_fn(|_| {
                    RwLock::new(UpstreamStatus {
                        available: false,
                        detail: Some("initial validation has not completed".to_owned()),
                    })
                }),
                generation: AtomicU64::new(1),
            }),
        }
    }
}

impl GateRegistry {
    pub fn status(&self) -> ValidatorStatus {
        ValidatorStatus {
            phase: *self.inner.phase.read(),
            verified: self.inner.verified.read().clone(),
            incident: self.inner.incident.read().clone(),
            neo_cli: self.inner.upstreams[0].read().clone(),
            neo_go: self.inner.upstreams[1].read().clone(),
            generation: self.inner.generation.load(Ordering::Acquire),
        }
    }

    pub fn begin_verifying(&self) {
        if *self.inner.phase.read() != ValidationPhase::Mismatch {
            *self.inner.phase.write() = ValidationPhase::Verifying;
            self.bump();
        }
    }

    pub fn mark_upstream(&self, kind: NodeKind, available: bool, detail: Option<String>) {
        let mut upstream = self.inner.upstreams[index(kind)].write();
        if upstream.available != available || upstream.detail != detail {
            *upstream = UpstreamStatus { available, detail };
            drop(upstream);
            self.bump();
        }
    }

    pub fn mark_verified(&self, point: VerifiedPoint) {
        *self.inner.verified.write() = Some(point);
        *self.inner.incident.write() = None;
        *self.inner.phase.write() = ValidationPhase::Healthy;
        for upstream in &self.inner.upstreams {
            *upstream.write() = UpstreamStatus {
                available: true,
                detail: None,
            };
        }
        self.bump();
    }

    pub fn quarantine(
        &self,
        phase: ValidationPhase,
        height: u32,
        code: impl Into<String>,
        detail: impl Into<String>,
    ) -> Incident {
        let code = code.into();
        let detail = detail.into();
        let generation = self.bump();
        let incident = Incident {
            id: format!("{}:{height}:{generation}", unix_millis()),
            height,
            code,
            detail: detail.clone(),
            observed_at_unix_ms: unix_millis(),
        };
        *self.inner.incident.write() = Some(incident.clone());
        *self.inner.phase.write() = phase;
        for upstream in &self.inner.upstreams {
            *upstream.write() = UpstreamStatus {
                available: false,
                detail: Some(detail.clone()),
            };
        }
        self.bump();
        incident
    }

    pub fn proxy_permit(&self, kind: NodeKind) -> Result<ProxyPermit> {
        let status = self.status();
        if status.phase != ValidationPhase::Healthy {
            bail!("validator is {:?}, not healthy", status.phase);
        }
        if !status.upstream(kind).available {
            bail!("upstream {} is quarantined", kind.as_str());
        }
        let verified = status
            .verified
            .ok_or_else(|| anyhow::anyhow!("no verified blockchain watermark"))?;
        Ok(ProxyPermit {
            generation: status.generation,
            verified_height: verified.height,
        })
    }

    pub fn permit_is_current(&self, kind: NodeKind, permit: ProxyPermit) -> bool {
        let status = self.status();
        status.generation == permit.generation
            && status.phase == ValidationPhase::Healthy
            && status.upstream(kind).available
            && status
                .verified
                .is_some_and(|point| point.height == permit.verified_height)
    }

    pub fn resume(
        &self,
        incident_id: &str,
        height: u32,
        hash: &UInt256,
        local_height: Option<u32>,
    ) -> Result<()> {
        let status = self.status();
        if !matches!(
            status.phase,
            ValidationPhase::Mismatch
                | ValidationPhase::CapabilityError
                | ValidationPhase::ExecutorError
        ) {
            bail!("no recoverable validation incident is active");
        }
        let incident = status
            .incident
            .ok_or_else(|| anyhow::anyhow!("active incident record is missing"))?;
        if incident.id != incident_id {
            bail!("incident id does not match the active incident");
        }
        let verified = status
            .verified
            .ok_or_else(|| anyhow::anyhow!("no verified recovery baseline exists"))?;
        if verified.height != height || &verified.block_hash != hash {
            bail!("recovery baseline does not match the verified watermark");
        }
        if local_height != Some(height) {
            bail!(
                "local executor height {local_height:?} differs from recovery baseline {height}; repair or rebuild validator storage instead of clearing the incident"
            );
        }
        *self.inner.phase.write() = ValidationPhase::Verifying;
        self.bump();
        Ok(())
    }

    fn bump(&self) -> u64 {
        self.inner.generation.fetch_add(1, Ordering::AcqRel) + 1
    }
}

impl ValidatorStatus {
    pub fn upstream(&self, kind: NodeKind) -> &UpstreamStatus {
        match kind {
            NodeKind::NeoCli => &self.neo_cli,
            NodeKind::NeoGo => &self.neo_go,
        }
    }
}

fn index(kind: NodeKind) -> usize {
    match kind {
        NodeKind::NeoCli => 0,
        NodeKind::NeoGo => 1,
    }
}

fn unix_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point() -> VerifiedPoint {
        VerifiedPoint {
            height: 7,
            block_hash: UInt256::zero(),
            state_root: UInt256::zero(),
        }
    }

    #[test]
    fn permit_is_revoked_when_incident_occurs_during_request() {
        let gates = GateRegistry::default();
        gates.mark_verified(point());
        let permit = gates.proxy_permit(NodeKind::NeoCli).unwrap();
        gates.quarantine(ValidationPhase::Mismatch, 8, "root_mismatch", "test");
        assert!(!gates.permit_is_current(NodeKind::NeoCli, permit));
    }

    #[test]
    fn recovery_requires_exact_incident_and_storage_baseline() {
        let gates = GateRegistry::default();
        gates.mark_verified(point());
        let incident = gates.quarantine(ValidationPhase::Mismatch, 8, "root_mismatch", "test");
        assert!(
            gates
                .resume(&incident.id, 7, &UInt256::zero(), Some(8))
                .is_err()
        );
        gates
            .resume(&incident.id, 7, &UInt256::zero(), Some(7))
            .unwrap();
        assert_eq!(gates.status().phase, ValidationPhase::Verifying);
        assert!(gates.status().incident.is_some(), "evidence is retained");
    }
}
