//! Compatibility paths and real-system integration tests for AI diagnostics.

pub(crate) use neo_ai_agent::AiAgentRuntime;

#[cfg(test)]
pub(crate) use crate::ai_agent_host::{canonical_block_hash, probe_session_config, runtime_host};
#[cfg(test)]
pub(crate) use crate::probe_controller::ProbeControllerConfig;
#[cfg(test)]
pub(crate) use async_trait::async_trait;
#[cfg(test)]
pub(crate) use neo_ai_agent::{
    ActionBinding, ActionResult, ActionState, AdmitOutcome, AnthropicProvider, AuditEventKind,
    AuditInput, AuditLog, AuthorizerMetadata, DiagnosisValidator, DiagnosticAgent,
    DiagnosticsSection, DurableActionStore, EvidenceAuthority, EvidenceBundle, EvidenceExporter,
    EvidenceItem, EvidenceKind, EvidenceReceiver, EvidenceRecord, EvidenceSource,
    EvidenceSubmitResult, ExecutionObservation, ExportResult, FileAuditLog, HttpProviderConfig,
    MemoryAuditLog, ModelProvider, ModelRequest, ModelResponse, NodeModelProvider, OllamaProvider,
    OpenAiCompatibleProvider, OperationId, OperationalAction, OperationalActionRequest,
    OperationalDecision, OperationalPolicy, OperationalPolicyConfig, OperationalSafetyState,
    PROBE_ACTION_CATALOG, PROBE_DETERMINISTIC_RULE_VERSION, ProbeAction, ProbeBudget, ProbeBuild,
    ProbeCompletion, ProbeContext, ProbeDeterministicRule, ProbeJob, ProbeResult,
    ProbeRuleFindingKind, ProbeTransaction, ProviderError, ProviderKind, RunId, StallDetector,
    StallPolicy, TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
    TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION, WorkerEvent, WorkerInput, action_request,
    advance_last_progress, build_model_provider, receive_worker_event, unix_time_ms,
};
#[cfg(test)]
pub(crate) use neo_primitives::UInt256;
#[cfg(test)]
pub(crate) use neo_system::{
    NeoSystem, TransactionExecutionObservation as SystemTransactionExecutionObservation,
};
#[cfg(test)]
pub(crate) use parking_lot::Mutex;
#[cfg(test)]
pub(crate) use serde_json::{Value, json};
#[cfg(test)]
pub(crate) use std::collections::{BTreeMap, BTreeSet, HashMap, VecDeque};
#[cfg(test)]
pub(crate) use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
#[cfg(test)]
pub(crate) use std::sync::{Arc, Weak};
#[cfg(test)]
pub(crate) use std::time::Duration;
#[cfg(test)]
pub(crate) use tokio::sync::{Notify, mpsc};
#[cfg(test)]
pub(crate) use tokio::task::JoinHandle;

#[cfg(test)]
pub(crate) trait AiAgentRuntimeNodeExt: Sized {
    fn new(section: &DiagnosticsSection, system: Arc<NeoSystem>) -> anyhow::Result<Self>;
    fn with_audit(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
    ) -> anyhow::Result<Self>;
    fn initialize(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
        action_store: Option<Arc<DurableActionStore>>,
    ) -> anyhow::Result<Self>;
}

#[cfg(test)]
impl AiAgentRuntimeNodeExt for AiAgentRuntime {
    fn new(section: &DiagnosticsSection, system: Arc<NeoSystem>) -> anyhow::Result<Self> {
        Self::new_with_host(section, runtime_host(system))
    }

    fn with_audit(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
    ) -> anyhow::Result<Self> {
        Self::initialize_with_host(section, runtime_host(system), audit, None)
    }

    fn initialize(
        section: &DiagnosticsSection,
        system: Arc<NeoSystem>,
        audit: Arc<dyn AuditLog>,
        action_store: Option<Arc<DurableActionStore>>,
    ) -> anyhow::Result<Self> {
        Self::initialize_with_host(section, runtime_host(system), audit, action_store)
    }
}

#[cfg(test)]
mod tests;
