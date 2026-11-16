//! Serializable diagnostics configuration shared by the node and probe controller.
//!
//! Phase 3 S6d2: these pure data sections sank from neo-node so both the
//! diagnostics runtime and the detached-probe controller can bind them without
//! depending on the assembly layer. No node or Neo protocol types are involved.

use crate::{CredentialEnv, ProviderKind, RuntimeLimits};
use serde::Deserialize;
use url::Url;

/// Optional real HTTP model provider for advisory-only diagnostics.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelProviderSection {
    /// Provider API and response schema.
    pub kind: ProviderKind,
    /// HTTPS endpoint, or HTTP only for a loopback provider.
    pub endpoint: Url,
    /// Remote or local model identifier.
    pub model: String,
    /// Environment variable containing the credential; never the credential itself.
    pub credential_env: Option<CredentialEnv>,
    /// Timeout, concurrency, retry, budget and circuit-breaker limits.
    pub limits: RuntimeLimits,
}

/// Capability-limited node diagnostics and bounded auto-action settings.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiagnosticsSection {
    /// Enables the diagnostic side-channel and worker.
    pub enabled: bool,
    /// Emergency switch that disables all operational actions.
    pub kill_switch: bool,
    /// Evaluates and audits actions without executing them.
    pub observe_only: bool,
    /// Enables structured action and worker audit events.
    pub audit_enabled: bool,
    /// Durable hash-chained JSONL audit path. Required when diagnostics are enabled.
    pub audit_path: String,
    /// Optional real model provider. Absence keeps model diagnosis unavailable.
    pub model_provider: Option<ModelProviderSection>,
    /// Bounded evidence queue capacity.
    pub evidence_queue_capacity: usize,
    /// Base node evidence sampling interval.
    pub sampling_interval_ms: u64,
    /// Maximum age considered fresh by health reporting.
    pub event_freshness_ms: u64,
    /// Maximum independent mempool quarantine records.
    pub quarantine_max_entries: usize,
    /// Maximum quarantine lifetime, capped at 30 seconds.
    pub quarantine_ttl_ms: u64,
    /// Explicitly enables bounded local transaction quarantine.
    pub allow_transaction_quarantine: bool,
    /// Maximum mempool entries scanned for one local proposal.
    pub proposal_scan_budget: usize,
    /// Deterministic quarantine rule version.
    pub rule_version: String,
    /// Enables otherwise disabled consensus restart policy evaluation.
    pub allow_consensus_restart: bool,
    /// Enables the restricted probe interface. It remains fail-closed without containment.
    pub restricted_probe_enabled: bool,
    /// Absolute path to the fixed `neo-probe` worker executable.
    pub restricted_probe_worker_path: String,
    /// Maximum number of queued probe requests.
    pub restricted_probe_queue_capacity: usize,
    /// Number of probe workers. Only one is permitted.
    pub restricted_probe_concurrency: usize,
    /// Maximum duration of one replay.
    pub restricted_probe_run_timeout_ms: u64,
    /// Maximum duration of the complete deterministic replay suite.
    pub restricted_probe_suite_timeout_ms: u64,
    /// Exact worker process memory ceiling.
    pub restricted_probe_memory_limit_bytes: u64,
    /// Maximum request or response frame size.
    pub restricted_probe_frame_limit_bytes: usize,
}

impl Default for DiagnosticsSection {
    fn default() -> Self {
        Self {
            enabled: false,
            kill_switch: false,
            observe_only: true,
            audit_enabled: true,
            audit_path: "data/diagnostics-audit.jsonl".to_string(),
            model_provider: None,
            evidence_queue_capacity: 1_024,
            sampling_interval_ms: 1_000,
            event_freshness_ms: 5_000,
            quarantine_max_entries: 1_024,
            quarantine_ttl_ms: 30_000,
            allow_transaction_quarantine: false,
            proposal_scan_budget: 2_048,
            rule_version: "node-diagnostics-v1".to_string(),
            allow_consensus_restart: false,
            restricted_probe_enabled: false,
            restricted_probe_worker_path: "neo-probe".to_string(),
            restricted_probe_queue_capacity: 16,
            restricted_probe_concurrency: 1,
            restricted_probe_run_timeout_ms: 3_000,
            restricted_probe_suite_timeout_ms: 7_000,
            restricted_probe_memory_limit_bytes: 256 * 1024 * 1024,
            restricted_probe_frame_limit_bytes: 512 * 1024,
        }
    }
}
