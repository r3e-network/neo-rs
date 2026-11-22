//! Bounded controller for isolated, deterministic `neo-probe` replays.

#![allow(dead_code)]

use crate::containment::{
    MAX_QUEUE_CAPACITY, MAX_RUN_TIMEOUT_MS, MAX_SUITE_TIMEOUT_MS, NativeProbeContainment,
    ProbeContainmentCapability, ProbeContainmentCapabilityProvider, ProbeContainmentProfile,
    REQUIRED_CONCURRENCY, REQUIRED_MEMORY_LIMIT_BYTES,
};
use crate::{MAX_FRAME_BYTES, ProbeBindings, ProbeRequestFrame, decode_frame, encode_frame};
use async_trait::async_trait;
use neo_ai_agent::{
    ActionBinding, AdmitOutcome, DiagnosticsSection, DurableActionStore, OperationId,
    OperationalAction, ProbeJob, ProbeOutcome, ProbeOutcomeStatus, ProbeResult,
    ProbeResultValidator, RunId, TrustedLocalDiagnosticsIdentity, unix_time_ms,
};
use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

const MAX_STDERR_BYTES: usize = 64 * 1024;
const PROBE_RATE_WINDOW_MS: u64 = 60_000;
const PROBE_GLOBAL_RATE_LIMIT: usize = 6;
const PROBE_TRANSACTION_COOLDOWN_MS: u64 = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeUncertainReason {
    Cancelled,
    ControllerClosed,
    DeadlineExceeded,
    SpawnFailed,
    ProcessIo,
    ProcessFailed,
    OutOfMemory,
    Protocol,
    BindingMismatch,
    ReplayMismatch,
    WorkerRejected,
    ContextChanged,
    ActionStore,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProbeVerdict {
    Completed(ProbeResult),
    Uncertain(ProbeUncertainReason),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeControllerResult {
    pub operation_id: OperationId,
    pub run_id: RunId,
    pub verdict: ProbeVerdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeSubmitError {
    Full,
    Closed,
    Invalid,
    GlobalRateLimited,
    TransactionCooldown,
}

#[derive(Debug)]
pub struct ProbeSubmission {
    operation_id: OperationId,
    run_id: RunId,
    cancellation: CancellationToken,
    receiver: Option<oneshot::Receiver<ProbeControllerResult>>,
}

impl ProbeSubmission {
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub async fn wait(mut self) -> ProbeControllerResult {
        let fallback = || ProbeControllerResult {
            operation_id: self.operation_id.clone(),
            run_id: self.run_id.clone(),
            verdict: ProbeVerdict::Uncertain(ProbeUncertainReason::ControllerClosed),
        };
        let Some(receiver) = self.receiver.take() else {
            return fallback();
        };
        receiver.await.unwrap_or_else(|_| fallback())
    }
}

impl Drop for ProbeSubmission {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

#[derive(Debug)]
struct WorkItem {
    request: ProbeRequestFrame,
    cancellation: CancellationToken,
    result: oneshot::Sender<ProbeControllerResult>,
}

#[derive(Debug)]
struct ControllerInner {
    sender: mpsc::Sender<WorkItem>,
    completions: Mutex<Option<mpsc::Receiver<ProbeControllerResult>>>,
    shutdown: CancellationToken,
    admission: Mutex<ProbeAdmissionState>,
}

impl Drop for ControllerInner {
    fn drop(&mut self) {
        self.shutdown.cancel();
    }
}

#[derive(Debug, Clone)]
pub struct ProbeController {
    inner: Arc<ControllerInner>,
}

impl ProbeController {
    pub fn start(
        config: ProbeControllerConfig,
        action_log: Arc<DurableActionStore>,
        context_rechecker: Arc<dyn ProbeContextRechecker>,
    ) -> Result<Self, ProbeStartupError> {
        Self::start_with_launcher(
            config,
            action_log,
            Arc::new(CommandProcessLauncher),
            context_rechecker,
            &NativeProbeContainment,
        )
    }

    fn start_with_launcher(
        config: ProbeControllerConfig,
        action_log: Arc<DurableActionStore>,
        launcher: Arc<dyn ProcessLauncher>,
        context_rechecker: Arc<dyn ProbeContextRechecker>,
        capabilities: &dyn ProbeContainmentCapabilityProvider,
    ) -> Result<Self, ProbeStartupError> {
        let config = validate_config_with_capability(config, capabilities)?;
        let (sender, receiver) = mpsc::channel(config.queue_capacity);
        let (completion_sender, completion_receiver) = mpsc::channel(config.queue_capacity);
        let shutdown = CancellationToken::new();
        tokio::spawn(worker_loop(
            receiver,
            config,
            action_log,
            launcher,
            context_rechecker,
            completion_sender,
            shutdown.clone(),
        ));
        Ok(Self {
            inner: Arc::new(ControllerInner {
                sender,
                completions: Mutex::new(Some(completion_receiver)),
                shutdown,
                admission: Mutex::new(ProbeAdmissionState::default()),
            }),
        })
    }

    pub fn try_submit(
        &self,
        request: ProbeRequestFrame,
    ) -> Result<ProbeSubmission, ProbeSubmitError> {
        validate_request_envelope(&request).map_err(|()| ProbeSubmitError::Invalid)?;
        match self
            .inner
            .admission
            .lock()
            .map_err(|_| ProbeSubmitError::Closed)?
            .admit(&request.job, unix_time_ms())
        {
            Ok(()) => {}
            Err(ProbeAdmissionError::GlobalRateLimited) => {
                return Err(ProbeSubmitError::GlobalRateLimited);
            }
            Err(ProbeAdmissionError::TransactionCooldown) => {
                return Err(ProbeSubmitError::TransactionCooldown);
            }
        }
        let operation_id = request.job.operation_id.clone();
        let run_id = request.job.run_id.clone();
        let cancellation = CancellationToken::new();
        let (result, receiver) = oneshot::channel();
        let item = WorkItem {
            request,
            cancellation: cancellation.clone(),
            result,
        };
        match self.inner.sender.try_send(item) {
            Ok(()) => Ok(ProbeSubmission {
                operation_id,
                run_id,
                cancellation,
                receiver: Some(receiver),
            }),
            Err(mpsc::error::TrySendError::Full(_)) => Err(ProbeSubmitError::Full),
            Err(mpsc::error::TrySendError::Closed(_)) => Err(ProbeSubmitError::Closed),
        }
    }

    /// Hands the bounded completion stream to exactly one runtime consumer.
    ///
    /// The receiver is handed out once; later callers get `None` so controller
    /// completions can never fan out to a second consumer.
    pub fn take_completions(&self) -> Option<mpsc::Receiver<ProbeControllerResult>> {
        self.inner
            .completions
            .lock()
            .ok()
            .and_then(|mut completions| completions.take())
    }
}

mod config;
mod runner;
mod worker;

pub use config::{
    ProbeAdmissionError, ProbeContextRechecker, ProbeControllerConfig, ProbeStartupError,
    probe_context_matches, validate_probe_startup,
};
use config::{ProbeAdmissionState, validate_config_with_capability};
use runner::{CommandProcessLauncher, ProcessLauncher};
use worker::{validate_request_envelope, worker_loop};

#[cfg(test)]
mod tests;
