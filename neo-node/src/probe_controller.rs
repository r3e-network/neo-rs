//! Bounded controller for isolated, deterministic `neo-probe` replays.

#![allow(dead_code)]

use crate::action_store::{ActionBinding, AdmitOutcome, DurableActionStore};
use crate::config::DiagnosticsSection;
use crate::probe_containment::{
    MAX_QUEUE_CAPACITY, MAX_RUN_TIMEOUT_MS, MAX_SUITE_TIMEOUT_MS, NativeProbeContainment,
    ProbeContainmentCapability, ProbeContainmentCapabilityProvider, ProbeContainmentProfile,
    REQUIRED_CONCURRENCY, REQUIRED_MEMORY_LIMIT_BYTES,
};
use async_trait::async_trait;
use neo_ai_agent::{
    OperationId, OperationalAction, ProbeJob, ProbeOutcome, ProbeOutcomeStatus, ProbeResult,
    ProbeResultValidator, RunId, TrustedLocalDiagnosticsIdentity,
};
use neo_probe::{MAX_FRAME_BYTES, ProbeBindings, ProbeRequestFrame, decode_frame, encode_frame};
use std::collections::{HashMap, VecDeque};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
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
pub(crate) enum ProbeAdmissionError {
    GlobalRateLimited,
    TransactionCooldown,
}

#[derive(Debug, Default)]
struct ProbeAdmissionState {
    admitted_at: VecDeque<u64>,
    transaction_last_admitted: HashMap<String, u64>,
}

impl ProbeAdmissionState {
    fn admit(&mut self, job: &ProbeJob, now_unix_ms: u64) -> Result<(), ProbeAdmissionError> {
        let window_start = now_unix_ms.saturating_sub(PROBE_RATE_WINDOW_MS);
        while self
            .admitted_at
            .front()
            .is_some_and(|admitted| *admitted <= window_start)
        {
            self.admitted_at.pop_front();
        }
        if self.admitted_at.len() >= PROBE_GLOBAL_RATE_LIMIT {
            return Err(ProbeAdmissionError::GlobalRateLimited);
        }
        let transaction_hash = &job.transaction.transaction_hash;
        if self
            .transaction_last_admitted
            .get(transaction_hash)
            .is_some_and(|last| now_unix_ms.saturating_sub(*last) < PROBE_TRANSACTION_COOLDOWN_MS)
        {
            return Err(ProbeAdmissionError::TransactionCooldown);
        }
        self.admitted_at.push_back(now_unix_ms);
        self.transaction_last_admitted
            .insert(transaction_hash.clone(), now_unix_ms);
        self.transaction_last_admitted.retain(|_, admitted| {
            now_unix_ms.saturating_sub(*admitted) < PROBE_TRANSACTION_COOLDOWN_MS
        });
        Ok(())
    }
}

pub(crate) trait ProbeContextRechecker: Send + Sync {
    fn is_current(&self, job: &ProbeJob) -> bool;
}

pub(crate) fn probe_context_matches(
    job: &ProbeJob,
    current_height: u32,
    current_tip_hash: &str,
) -> bool {
    job.context.observed_height == current_height && job.context.tip_hash == current_tip_hash
}

#[derive(Debug, Clone)]
pub(crate) struct ProbeControllerConfig {
    pub(crate) executable: PathBuf,
    pub(crate) queue_capacity: usize,
    pub(crate) concurrency: usize,
    pub(crate) replay_timeout: Duration,
    pub(crate) suite_timeout: Duration,
    pub(crate) memory_limit_bytes: u64,
    pub(crate) frame_limit_bytes: usize,
}

impl From<&DiagnosticsSection> for ProbeControllerConfig {
    fn from(section: &DiagnosticsSection) -> Self {
        Self {
            executable: PathBuf::from(&section.restricted_probe_worker_path),
            queue_capacity: section.restricted_probe_queue_capacity,
            concurrency: section.restricted_probe_concurrency,
            replay_timeout: Duration::from_millis(section.restricted_probe_run_timeout_ms),
            suite_timeout: Duration::from_millis(section.restricted_probe_suite_timeout_ms),
            memory_limit_bytes: section.restricted_probe_memory_limit_bytes,
            frame_limit_bytes: section.restricted_probe_frame_limit_bytes,
        }
    }
}

impl ProbeControllerConfig {
    fn profile(&self) -> ProbeContainmentProfile {
        ProbeContainmentProfile {
            name: crate::probe_containment::RESTRICTED_PROBE_PROFILE,
            memory_limit_bytes: self.memory_limit_bytes,
            run_timeout_ms: u64::try_from(self.replay_timeout.as_millis()).unwrap_or(u64::MAX),
            suite_timeout_ms: u64::try_from(self.suite_timeout.as_millis()).unwrap_or(u64::MAX),
        }
    }

    fn validate(mut self) -> Result<Self, ProbeStartupError> {
        if self.queue_capacity == 0 || self.queue_capacity > MAX_QUEUE_CAPACITY {
            return Err(ProbeStartupError::InvalidConfig(
                "restricted probe queue capacity must be between 1 and 16",
            ));
        }
        if self.concurrency != REQUIRED_CONCURRENCY {
            return Err(ProbeStartupError::InvalidConfig(
                "restricted probe concurrency must be exactly 1",
            ));
        }
        let run_ms = u64::try_from(self.replay_timeout.as_millis()).unwrap_or(u64::MAX);
        let suite_ms = u64::try_from(self.suite_timeout.as_millis()).unwrap_or(u64::MAX);
        if run_ms == 0 || run_ms > MAX_RUN_TIMEOUT_MS {
            return Err(ProbeStartupError::InvalidConfig(
                "restricted probe run timeout must be between 1 and 3000 ms",
            ));
        }
        if suite_ms < run_ms.saturating_mul(2) || suite_ms > MAX_SUITE_TIMEOUT_MS {
            return Err(ProbeStartupError::InvalidConfig(
                "restricted probe suite timeout must cover two runs and be at most 7000 ms",
            ));
        }
        if self.memory_limit_bytes != REQUIRED_MEMORY_LIMIT_BYTES {
            return Err(ProbeStartupError::InvalidConfig(
                "restricted probe memory limit must be exactly 256 MiB",
            ));
        }
        if self.frame_limit_bytes == 0 || self.frame_limit_bytes > MAX_FRAME_BYTES {
            return Err(ProbeStartupError::InvalidConfig(
                "restricted probe frame limit must be between 1 and 512 KiB",
            ));
        }
        self.executable = validate_worker_path(&self.executable)?;
        Ok(self)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum ProbeStartupError {
    #[error("invalid restricted probe configuration: {0}")]
    InvalidConfig(&'static str),
    #[error("restricted probe worker path is invalid: {0}")]
    InvalidWorkerPath(&'static str),
    #[error("restricted probe containment is unsupported: {0}")]
    UnsupportedContainment(String),
    #[error("restricted probe capability does not match the requested profile")]
    CapabilityProfileMismatch,
}

fn validate_worker_path(path: &Path) -> Result<PathBuf, ProbeStartupError> {
    let raw = path.to_string_lossy();
    if raw.trim().is_empty()
        || raw.contains("\0")
        || raw.to_ascii_lowercase().contains("://")
        || !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(ProbeStartupError::InvalidWorkerPath(
            "an absolute local path without traversal or URL syntax is required",
        ));
    }
    let expected_name = if cfg!(windows) {
        "neo-probe.exe"
    } else {
        "neo-probe"
    };
    if path.file_name().and_then(|name| name.to_str()) != Some(expected_name) {
        return Err(ProbeStartupError::InvalidWorkerPath(
            "the executable name must be the fixed neo-probe worker name",
        ));
    }
    let canonical = std::fs::canonicalize(path).map_err(|_| {
        ProbeStartupError::InvalidWorkerPath("the worker must exist and be canonicalizable")
    })?;
    if !canonical.is_file()
        || canonical.file_name().and_then(|name| name.to_str()) != Some(expected_name)
    {
        return Err(ProbeStartupError::InvalidWorkerPath(
            "the canonical worker target must be the fixed neo-probe executable",
        ));
    }
    Ok(canonical)
}

pub(crate) fn validate_probe_startup(
    section: &DiagnosticsSection,
    capabilities: &dyn ProbeContainmentCapabilityProvider,
) -> Result<ProbeControllerConfig, ProbeStartupError> {
    validate_config_with_capability(ProbeControllerConfig::from(section), capabilities)
}

fn validate_config_with_capability(
    config: ProbeControllerConfig,
    capabilities: &dyn ProbeContainmentCapabilityProvider,
) -> Result<ProbeControllerConfig, ProbeStartupError> {
    let config = config.validate()?;
    let profile = config.profile();
    match capabilities.capability(&profile) {
        ProbeContainmentCapability::Supported(evidence) => {
            if evidence.profile != profile || !evidence.is_complete() {
                return Err(ProbeStartupError::CapabilityProfileMismatch);
            }
        }
        ProbeContainmentCapability::Unsupported(evidence) => {
            return Err(ProbeStartupError::UnsupportedContainment(format!(
                "{} missing {:?}",
                evidence.mechanism, evidence.missing
            )));
        }
    }
    Ok(config)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeUncertainReason {
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
pub(crate) enum ProbeVerdict {
    Completed(ProbeResult),
    Uncertain(ProbeUncertainReason),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProbeControllerResult {
    pub(crate) operation_id: OperationId,
    pub(crate) run_id: RunId,
    pub(crate) verdict: ProbeVerdict,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProbeSubmitError {
    Full,
    Closed,
    Invalid,
    GlobalRateLimited,
    TransactionCooldown,
}

#[derive(Debug)]
pub(crate) struct ProbeSubmission {
    operation_id: OperationId,
    run_id: RunId,
    cancellation: CancellationToken,
    receiver: Option<oneshot::Receiver<ProbeControllerResult>>,
}

impl ProbeSubmission {
    pub(crate) fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub(crate) async fn wait(mut self) -> ProbeControllerResult {
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
pub(crate) struct ProbeController {
    inner: Arc<ControllerInner>,
}

impl ProbeController {
    pub(crate) fn start(
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

    pub(crate) fn try_submit(
        &self,
        request: ProbeRequestFrame,
    ) -> Result<ProbeSubmission, ProbeSubmitError> {
        validate_request_envelope(&request).map_err(|_| ProbeSubmitError::Invalid)?;
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
    pub(crate) fn take_completions(&self) -> Option<mpsc::Receiver<ProbeControllerResult>> {
        self.inner
            .completions
            .lock()
            .ok()
            .and_then(|mut completions| completions.take())
    }
}

fn validate_request_envelope(request: &ProbeRequestFrame) -> Result<(), ()> {
    request.job.validate().map_err(|_| ())?;
    let expected = ProbeBindings::calculate(&request.job).map_err(|_| ())?;
    if request.bindings != expected
        || request.snapshot.root_hash != request.job.context.tip_hash
        || request.snapshot.height != request.job.context.observed_height
    {
        return Err(());
    }
    encode_frame(request).map_err(|_| ())?;
    Ok(())
}

async fn worker_loop(
    mut receiver: mpsc::Receiver<WorkItem>,
    config: ProbeControllerConfig,
    action_log: Arc<DurableActionStore>,
    launcher: Arc<dyn ProcessLauncher>,
    context_rechecker: Arc<dyn ProbeContextRechecker>,
    completions: mpsc::Sender<ProbeControllerResult>,
    shutdown: CancellationToken,
) {
    loop {
        let item = tokio::select! {
            biased;
            () = shutdown.cancelled() => {
                receiver.close();
                while let Ok(item) = receiver.try_recv() {
                    close_queued_item(&action_log, item);
                }
                break;
            }
            item = receiver.recv() => match item {
                Some(item) => item,
                None => break,
            },
        };
        let result = execute_item(
            &config,
            &action_log,
            launcher.as_ref(),
            context_rechecker.as_ref(),
            &shutdown,
            &item,
        )
        .await;
        // Bounded, non-blocking completion fan-out for the runtime consumer:
        // a full completion queue drops the notification instead of blocking
        // the single controller worker here.
        let _ = completions.try_send(result.clone());
        let _ = item.result.send(result);
    }
}

fn close_queued_item(action_log: &DurableActionStore, item: WorkItem) {
    let job = &item.request.job;
    if begin_action(action_log, job).is_ok() {
        mark_uncertain(action_log, &job.operation_id);
    }
    let _ = item.result.send(ProbeControllerResult {
        operation_id: job.operation_id.clone(),
        run_id: job.run_id.clone(),
        verdict: ProbeVerdict::Uncertain(ProbeUncertainReason::ControllerClosed),
    });
}

async fn execute_item(
    config: &ProbeControllerConfig,
    action_log: &DurableActionStore,
    launcher: &dyn ProcessLauncher,
    context_rechecker: &dyn ProbeContextRechecker,
    shutdown: &CancellationToken,
    item: &WorkItem,
) -> ProbeControllerResult {
    let job = &item.request.job;
    let uncertain = |reason| ProbeControllerResult {
        operation_id: job.operation_id.clone(),
        run_id: job.run_id.clone(),
        verdict: ProbeVerdict::Uncertain(reason),
    };

    if !context_rechecker.is_current(job) {
        return uncertain(ProbeUncertainReason::ContextChanged);
    }
    if begin_action(action_log, job).is_err() {
        return uncertain(ProbeUncertainReason::ActionStore);
    }

    let framed = match encode_frame(&item.request) {
        Ok(framed) => framed,
        Err(_) => {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::Protocol);
        }
    };
    let suite_deadline = Instant::now() + config.suite_timeout;
    let cancellation = RunCancellation {
        controller: shutdown.clone(),
        job: item.cancellation.clone(),
    };
    let mut outcomes = Vec::with_capacity(2);
    for _ in 0..2 {
        let output = match launcher
            .run(
                &config.executable,
                framed.clone(),
                config.replay_timeout,
                suite_deadline,
                config.memory_limit_bytes,
                config.frame_limit_bytes,
                cancellation.clone(),
            )
            .await
        {
            Ok(output) => output,
            Err(error) => {
                mark_uncertain(action_log, &job.operation_id);
                return uncertain(error.reason());
            }
        };
        if oom_indicator(&output.stderr) {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::OutOfMemory);
        }
        if !output.success {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::ProcessFailed);
        }
        let outcome = match decode_frame::<ProbeOutcome>(&output.stdout) {
            Ok(outcome) => outcome,
            Err(_) => {
                mark_uncertain(action_log, &job.operation_id);
                return uncertain(ProbeUncertainReason::Protocol);
            }
        };
        if outcome.status != ProbeOutcomeStatus::Succeeded {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::WorkerRejected);
        }
        outcomes.push(outcome);
    }

    if outcomes[0] != outcomes[1] {
        mark_uncertain(action_log, &job.operation_id);
        return uncertain(ProbeUncertainReason::ReplayMismatch);
    }
    let identity = match TrustedLocalDiagnosticsIdentity::from_job(job) {
        Ok(identity) => identity,
        Err(_) => {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::BindingMismatch);
        }
    };
    let validated = match ProbeResultValidator.validate_double_replay(
        &identity,
        job,
        outcomes.remove(0),
        outcomes.remove(0),
        unix_time_ms(),
    ) {
        Ok(validated) => validated,
        Err(_) => {
            mark_uncertain(action_log, &job.operation_id);
            return uncertain(ProbeUncertainReason::BindingMismatch);
        }
    };
    if complete_action(action_log, &job.operation_id).is_err() {
        return uncertain(ProbeUncertainReason::ActionStore);
    }
    ProbeControllerResult {
        operation_id: job.operation_id.clone(),
        run_id: job.run_id.clone(),
        verdict: ProbeVerdict::Completed(validated),
    }
}

fn begin_action(action_log: &DurableActionStore, job: &ProbeJob) -> Result<(), ()> {
    let binding_hash = job.binding_hash().map_err(|_| ())?;
    let binding = ActionBinding::new(
        job.operation_id.clone(),
        OperationalAction::StartProbeRequest,
        Some(format!("{}:{binding_hash}", job.run_id)),
        vec![job.authorizer.evidence_id.clone()],
        vec![job.authorizer.evidence_hash.clone()],
        job.authorizer.rule_version.clone(),
    )
    .map_err(|_| ())?;
    match action_log.admit(binding, unix_time_ms()).map_err(|_| ())? {
        AdmitOutcome::Admitted(_) => {}
        AdmitOutcome::Existing(_) => return Err(()),
    }
    action_log
        .mark_running(&job.operation_id, unix_time_ms())
        .map_err(|_| ())?;
    Ok(())
}

fn complete_action(action_log: &DurableActionStore, operation_id: &OperationId) -> Result<(), ()> {
    if action_log
        .mark_completed(operation_id, unix_time_ms())
        .is_err()
    {
        mark_uncertain(action_log, operation_id);
        return Err(());
    }
    Ok(())
}

fn mark_uncertain(action_log: &DurableActionStore, operation_id: &OperationId) {
    let _ = action_log.mark_uncertain(operation_id, unix_time_ms());
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(1)
        .max(1)
}

fn oom_indicator(stderr: &[u8]) -> bool {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    [
        "out of memory",
        "cannot allocate memory",
        "memory allocation failed",
        "oom",
        "status_no_memory",
    ]
    .iter()
    .any(|indicator| stderr.contains(indicator))
}

#[derive(Debug, Clone)]
struct RunCancellation {
    controller: CancellationToken,
    job: CancellationToken,
}

#[derive(Debug)]
struct ProcessOutput {
    success: bool,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LaunchError {
    Cancelled,
    Deadline,
    Spawn,
    Containment,
    Io,
    OutputTooLarge,
}

impl LaunchError {
    const fn reason(self) -> ProbeUncertainReason {
        match self {
            Self::Cancelled => ProbeUncertainReason::Cancelled,
            Self::Deadline => ProbeUncertainReason::DeadlineExceeded,
            Self::Spawn | Self::Containment => ProbeUncertainReason::SpawnFailed,
            Self::Io => ProbeUncertainReason::ProcessIo,
            Self::OutputTooLarge => ProbeUncertainReason::Protocol,
        }
    }
}

#[async_trait]
trait ProcessLauncher: Send + Sync {
    async fn run(
        &self,
        executable: &PathBuf,
        stdin: Vec<u8>,
        replay_timeout: Duration,
        suite_deadline: Instant,
        memory_limit_bytes: u64,
        frame_limit_bytes: usize,
        cancellation: RunCancellation,
    ) -> Result<ProcessOutput, LaunchError>;
}

#[derive(Debug, Clone, Copy)]
struct CommandProcessLauncher;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerHandlePolicy {
    ProtocolStdioOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WorkerLaunchPolicy {
    empty_environment: bool,
    handle_policy: WorkerHandlePolicy,
    argument_count: usize,
}

const WORKER_LAUNCH_POLICY: WorkerLaunchPolicy = WorkerLaunchPolicy {
    empty_environment: true,
    handle_policy: WorkerHandlePolicy::ProtocolStdioOnly,
    argument_count: 0,
};

fn restricted_worker_command(executable: &Path) -> Command {
    let mut command = Command::new(executable);
    if WORKER_LAUNCH_POLICY.empty_environment {
        command.env_clear();
    }
    match WORKER_LAUNCH_POLICY.handle_policy {
        WorkerHandlePolicy::ProtocolStdioOnly => {
            command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        }
    }
    debug_assert_eq!(WORKER_LAUNCH_POLICY.argument_count, 0);
    command.kill_on_drop(true);
    command
}

#[cfg(windows)]
#[derive(Debug)]
struct WindowsJob(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
// SAFETY: The owned Job Object handle may be closed from any thread.
unsafe impl Send for WindowsJob {}

#[cfg(windows)]
impl WindowsJob {
    fn assign(
        child: &tokio::process::Child,
        memory_limit_bytes: u64,
        replay_timeout: Duration,
    ) -> std::io::Result<Self> {
        use std::mem::size_of;
        use windows_sys::Win32::System::JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
            JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOB_OBJECT_LIMIT_PROCESS_TIME,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
            SetInformationJobObject,
        };

        // SAFETY: Null security attributes and name request a private, non-inheritable job.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Self(handle);
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE
            | JOB_OBJECT_LIMIT_ACTIVE_PROCESS
            | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION
            | JOB_OBJECT_LIMIT_PROCESS_MEMORY
            | JOB_OBJECT_LIMIT_PROCESS_TIME;
        limits.BasicLimitInformation.ActiveProcessLimit = 1;
        limits.BasicLimitInformation.PerProcessUserTimeLimit =
            i64::try_from(replay_timeout.as_millis().saturating_mul(10_000)).unwrap_or(i64::MAX);
        limits.ProcessMemoryLimit = usize::try_from(memory_limit_bytes).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "memory limit does not fit usize",
            )
        })?;
        // SAFETY: `limits` is initialized for the documented information class.
        let configured = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&limits).cast(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .unwrap_or(u32::MAX),
            )
        };
        if configured == 0 {
            return Err(std::io::Error::last_os_error());
        }
        let process = child.raw_handle().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "worker exited before job assignment",
            )
        })?;
        // SAFETY: Tokio exposes the live owned process handle for this child.
        let assigned = unsafe { AssignProcessToJobObject(job.0, process.cast()) };
        if assigned == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(job)
    }
}

#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        // SAFETY: This type uniquely owns the Job Object handle.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}

#[async_trait]
impl ProcessLauncher for CommandProcessLauncher {
    async fn run(
        &self,
        executable: &PathBuf,
        stdin: Vec<u8>,
        replay_timeout: Duration,
        suite_deadline: Instant,
        memory_limit_bytes: u64,
        frame_limit_bytes: usize,
        cancellation: RunCancellation,
    ) -> Result<ProcessOutput, LaunchError> {
        let mut child = restricted_worker_command(executable)
            .spawn()
            .map_err(|_| LaunchError::Spawn)?;
        #[cfg(windows)]
        let _job = WindowsJob::assign(&child, memory_limit_bytes, replay_timeout)
            .map_err(|_| LaunchError::Containment)?;
        #[cfg(not(windows))]
        let _ = memory_limit_bytes;
        let Some(mut child_stdin) = child.stdin.take() else {
            terminate_and_reap(&mut child).await;
            return Err(LaunchError::Io);
        };
        let Some(child_stdout) = child.stdout.take() else {
            terminate_and_reap(&mut child).await;
            return Err(LaunchError::Io);
        };
        let Some(child_stderr) = child.stderr.take() else {
            terminate_and_reap(&mut child).await;
            return Err(LaunchError::Io);
        };
        let writer = tokio::spawn(async move {
            child_stdin.write_all(&stdin).await?;
            child_stdin.shutdown().await
        });
        let stdout = tokio::spawn(read_limited(child_stdout, frame_limit_bytes + 4));
        let stderr = tokio::spawn(read_limited(child_stderr, MAX_STDERR_BYTES));
        let replay_deadline = Instant::now() + replay_timeout;
        let deadline = replay_deadline.min(suite_deadline);
        let status = tokio::select! {
            biased;
            () = cancellation.controller.cancelled() => {
                terminate_and_reap(&mut child).await;
                abort_io(writer, stdout, stderr).await;
                return Err(LaunchError::Cancelled);
            }
            () = cancellation.job.cancelled() => {
                terminate_and_reap(&mut child).await;
                abort_io(writer, stdout, stderr).await;
                return Err(LaunchError::Cancelled);
            }
            () = tokio::time::sleep_until(deadline) => {
                terminate_and_reap(&mut child).await;
                abort_io(writer, stdout, stderr).await;
                return Err(LaunchError::Deadline);
            }
            status = child.wait() => match status {
                Ok(status) => status,
                Err(_) => {
                    terminate_and_reap(&mut child).await;
                    abort_io(writer, stdout, stderr).await;
                    return Err(LaunchError::Io);
                }
            },
        };
        let writer_result = writer.await;
        let stdout_result = stdout.await;
        let stderr_result = stderr.await;
        let writer_ok = matches!(writer_result, Ok(Ok(())));
        let stdout = match stdout_result {
            Ok(Ok(stdout)) => stdout,
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err(LaunchError::Io),
        };
        let stderr = match stderr_result {
            Ok(Ok(stderr)) => stderr,
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err(LaunchError::Io),
        };
        if !writer_ok {
            return Err(LaunchError::Io);
        }
        Ok(ProcessOutput {
            success: status.success(),
            stdout,
            stderr,
        })
    }
}

async fn read_limited(
    reader: impl AsyncRead + Unpin,
    limit: usize,
) -> Result<Vec<u8>, LaunchError> {
    let mut bytes = Vec::new();
    let mut limited = reader.take(u64::try_from(limit + 1).unwrap_or(u64::MAX));
    limited
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| LaunchError::Io)?;
    if bytes.len() > limit {
        return Err(LaunchError::OutputTooLarge);
    }
    Ok(bytes)
}

async fn terminate_and_reap(child: &mut tokio::process::Child) {
    let _ = child.start_kill();
    let _ = child.wait().await;
}

async fn abort_io(
    writer: tokio::task::JoinHandle<Result<(), std::io::Error>>,
    stdout: tokio::task::JoinHandle<Result<Vec<u8>, LaunchError>>,
    stderr: tokio::task::JoinHandle<Result<Vec<u8>, LaunchError>>,
) {
    writer.abort();
    stdout.abort();
    stderr.abort();
    let _ = writer.await;
    let _ = stdout.await;
    let _ = stderr.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe_containment::{
        ContainmentProperty, FixedProbeContainment, ProbeContainmentEvidence,
    };
    use neo_ai_agent::{
        AuthorizerMetadata, EvidenceAuthority, ProbeAction, ProbeBudget, ProbeBuild, ProbeContext,
        ProbeTransaction,
    };
    use neo_probe::{SnapshotEntry, SnapshotInput};
    use serde_json::json;
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, VecDeque};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tempfile::TempDir;
    use tokio::sync::Notify;

    #[derive(Clone)]
    enum FakeBehavior {
        Output {
            bytes: Vec<u8>,
            stderr: Vec<u8>,
            success: bool,
        },
        Delay(Duration, Vec<u8>),
    }

    #[derive(Debug)]
    struct FixedContextRechecker {
        height: u32,
        tip_hash: String,
    }

    impl ProbeContextRechecker for FixedContextRechecker {
        fn is_current(&self, job: &ProbeJob) -> bool {
            probe_context_matches(job, self.height, &self.tip_hash)
        }
    }

    fn matching_context_rechecker() -> Arc<dyn ProbeContextRechecker> {
        Arc::new(FixedContextRechecker {
            height: 30,
            tip_hash: "ab".repeat(32),
        })
    }

    struct ActiveGuard<'a> {
        launcher: &'a FakeLauncher,
    }

    impl Drop for ActiveGuard<'_> {
        fn drop(&mut self) {
            self.launcher.active.fetch_sub(1, Ordering::SeqCst);
            self.launcher.reaped.fetch_add(1, Ordering::SeqCst);
        }
    }

    struct FakeLauncher {
        behaviors: Mutex<VecDeque<FakeBehavior>>,
        calls: AtomicUsize,
        active: AtomicUsize,
        max_active: AtomicUsize,
        reaped: AtomicUsize,
        started: Notify,
    }

    impl FakeLauncher {
        fn new(behaviors: impl IntoIterator<Item = FakeBehavior>) -> Arc<Self> {
            Arc::new(Self {
                behaviors: Mutex::new(behaviors.into_iter().collect()),
                calls: AtomicUsize::new(0),
                active: AtomicUsize::new(0),
                max_active: AtomicUsize::new(0),
                reaped: AtomicUsize::new(0),
                started: Notify::new(),
            })
        }

        async fn wait_for_calls(&self, expected: usize) {
            while self.calls.load(Ordering::SeqCst) < expected {
                self.started.notified().await;
            }
        }
    }

    #[async_trait]
    impl ProcessLauncher for FakeLauncher {
        async fn run(
            &self,
            _executable: &PathBuf,
            _stdin: Vec<u8>,
            replay_timeout: Duration,
            suite_deadline: Instant,
            _memory_limit_bytes: u64,
            _frame_limit_bytes: usize,
            cancellation: RunCancellation,
        ) -> Result<ProcessOutput, LaunchError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
            self.max_active.fetch_max(active, Ordering::SeqCst);
            self.started.notify_waiters();
            let _guard = ActiveGuard { launcher: self };
            let behavior =
                self.behaviors
                    .lock()
                    .unwrap()
                    .pop_front()
                    .unwrap_or(FakeBehavior::Output {
                        bytes: Vec::new(),
                        stderr: Vec::new(),
                        success: false,
                    });
            let deadline = (Instant::now() + replay_timeout).min(suite_deadline);
            match behavior {
                FakeBehavior::Output {
                    bytes,
                    stderr,
                    success,
                } => Ok(ProcessOutput {
                    success,
                    stdout: bytes,
                    stderr,
                }),
                FakeBehavior::Delay(delay, bytes) => tokio::select! {
                    biased;
                    () = cancellation.controller.cancelled() => Err(LaunchError::Cancelled),
                    () = cancellation.job.cancelled() => Err(LaunchError::Cancelled),
                    () = tokio::time::sleep_until(deadline) => Err(LaunchError::Deadline),
                    () = tokio::time::sleep(delay) => Ok(ProcessOutput {
                        success: true,
                        stdout: bytes,
                        stderr: Vec::new(),
                    }),
                },
            }
        }
    }

    fn hash(bytes: &[u8]) -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect()
    }

    fn request(id: usize) -> ProbeRequestFrame {
        let bytes = format!("task-30-transaction-{id}").into_bytes();
        let job = ProbeJob::new(
            OperationId::new(format!("operation-30-{id}")).unwrap(),
            RunId::new(format!("run-{id}")).unwrap(),
            ProbeAction::TransactionPreflight,
            ProbeTransaction {
                transaction_hash: hash(&bytes),
                transaction_bytes: bytes,
            },
            ProbeContext {
                observed_height: 30,
                tip_hash: "ab".repeat(32),
                requested_at_unix_ms: 1_000,
                node_instance_id: "probe-controller-test".to_owned(),
            },
            ProbeBuild {
                binary_hash: "bc".repeat(32),
                version: "neo-probe-0.20.0".to_owned(),
            },
            ProbeBudget {
                max_duration_ms: 1_000,
                max_output_bytes: 4_096,
                max_observations: 8,
            },
            AuthorizerMetadata {
                evidence_id: format!("task-30-rule-{id}"),
                evidence_hash: "cd".repeat(32),
                authority: EvidenceAuthority::DeterministicRule,
                rule_version: "task-30-v1".to_owned(),
                authorized_at_unix_ms: 999,
            },
        )
        .unwrap();
        let snapshot = SnapshotInput::from_job(
            &job,
            vec![SnapshotEntry {
                key: vec![1, 2, 3],
                value: vec![4, 5, 6],
                constant: false,
            }],
        );
        ProbeRequestFrame::new(job, snapshot).unwrap()
    }

    fn outcome(request: &ProbeRequestFrame) -> ProbeOutcome {
        ProbeOutcome {
            schema_version: neo_ai_agent::PROBE_SCHEMA_VERSION,
            operation_id: request.job.operation_id.clone(),
            run_id: request.job.run_id.clone(),
            action: request.job.action,
            job_hash: request.job.binding_hash().unwrap(),
            reported_authority: EvidenceAuthority::Observation,
            status: ProbeOutcomeStatus::Succeeded,
            started_at_unix_ms: unix_time_ms(),
            completed_at_unix_ms: unix_time_ms(),
            observations: BTreeMap::from([("preflight".to_owned(), json!({"accepted": true}))]),
        }
    }

    fn framed(request: &ProbeRequestFrame) -> Vec<u8> {
        encode_frame(&outcome(request)).unwrap()
    }

    fn output(bytes: Vec<u8>) -> FakeBehavior {
        FakeBehavior::Output {
            bytes,
            stderr: Vec::new(),
            success: true,
        }
    }

    fn setup(
        capacity: usize,
        behaviors: impl IntoIterator<Item = FakeBehavior>,
        replay_timeout: Duration,
    ) -> (
        TempDir,
        ProbeController,
        Arc<FakeLauncher>,
        Arc<DurableActionStore>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let log =
            Arc::new(DurableActionStore::open(directory.path().join("actions.jsonl")).unwrap());
        let launcher = FakeLauncher::new(behaviors);
        let executable = directory.path().join(if cfg!(windows) {
            "neo-probe.exe"
        } else {
            "neo-probe"
        });
        std::fs::write(&executable, b"trusted test worker").unwrap();
        let config = ProbeControllerConfig {
            executable,
            queue_capacity: capacity,
            concurrency: REQUIRED_CONCURRENCY,
            replay_timeout,
            suite_timeout: replay_timeout
                .saturating_mul(3)
                .min(Duration::from_millis(MAX_SUITE_TIMEOUT_MS)),
            memory_limit_bytes: REQUIRED_MEMORY_LIMIT_BYTES,
            frame_limit_bytes: MAX_FRAME_BYTES,
        };
        let capabilities = FixedProbeContainment(ProbeContainmentCapability::trusted_for_test(
            config.profile(),
        ));
        let controller = ProbeController::start_with_launcher(
            config,
            log.clone(),
            launcher.clone(),
            matching_context_rechecker(),
            &capabilities,
        )
        .unwrap();
        (directory, controller, launcher, log)
    }

    fn valid_test_config(directory: &TempDir) -> ProbeControllerConfig {
        let executable = directory.path().join(if cfg!(windows) {
            "neo-probe.exe"
        } else {
            "neo-probe"
        });
        std::fs::write(&executable, b"trusted test worker").unwrap();
        ProbeControllerConfig {
            executable,
            queue_capacity: MAX_QUEUE_CAPACITY,
            concurrency: REQUIRED_CONCURRENCY,
            replay_timeout: Duration::from_millis(MAX_RUN_TIMEOUT_MS),
            suite_timeout: Duration::from_millis(MAX_SUITE_TIMEOUT_MS),
            memory_limit_bytes: REQUIRED_MEMORY_LIMIT_BYTES,
            frame_limit_bytes: MAX_FRAME_BYTES,
        }
    }

    fn test_log(directory: &TempDir) -> Arc<DurableActionStore> {
        Arc::new(DurableActionStore::open(directory.path().join("actions.jsonl")).unwrap())
    }

    #[test]
    fn unsupported_capability_rejects_startup_without_fallback() {
        let directory = tempfile::tempdir().unwrap();
        let config = valid_test_config(&directory);
        let evidence = ProbeContainmentEvidence {
            profile: config.profile(),
            proven: Vec::new(),
            missing: vec![ContainmentProperty::NetworkIsolation],
            mechanism: "unsupported-test-provider",
        };
        let capabilities = FixedProbeContainment(ProbeContainmentCapability::Unsupported(evidence));
        let result = ProbeController::start_with_launcher(
            config,
            test_log(&directory),
            FakeLauncher::new([]),
            matching_context_rechecker(),
            &capabilities,
        );
        assert!(matches!(
            result,
            Err(ProbeStartupError::UnsupportedContainment(_))
        ));
    }

    #[tokio::test]
    async fn trusted_complete_capability_can_start_controller() {
        let directory = tempfile::tempdir().unwrap();
        let config = valid_test_config(&directory);
        let capabilities = FixedProbeContainment(ProbeContainmentCapability::trusted_for_test(
            config.profile(),
        ));
        let controller = ProbeController::start_with_launcher(
            config,
            test_log(&directory),
            FakeLauncher::new([]),
            matching_context_rechecker(),
            &capabilities,
        )
        .expect("trusted capability should open the startup gate");
        drop(controller);
    }

    #[test]
    fn mismatched_capability_profile_is_rejected() {
        let directory = tempfile::tempdir().unwrap();
        let config = valid_test_config(&directory);
        let mut wrong = config.profile();
        wrong.memory_limit_bytes -= 1;
        let capabilities =
            FixedProbeContainment(ProbeContainmentCapability::trusted_for_test(wrong));
        let result = ProbeController::start_with_launcher(
            config,
            test_log(&directory),
            FakeLauncher::new([]),
            matching_context_rechecker(),
            &capabilities,
        );
        assert!(matches!(
            result,
            Err(ProbeStartupError::CapabilityProfileMismatch)
        ));
    }

    #[test]
    fn bad_worker_paths_and_url_syntax_are_rejected() {
        let directory = tempfile::tempdir().unwrap();
        for path in [
            PathBuf::from("neo-probe"),
            PathBuf::from("https://example.invalid/neo-probe.exe"),
            directory.path().join("arbitrary-worker.exe"),
        ] {
            let mut config = valid_test_config(&directory);
            config.executable = path;
            assert!(matches!(
                config.validate(),
                Err(ProbeStartupError::InvalidWorkerPath(_))
            ));
        }
    }

    #[test]
    fn budget_boundaries_reject_out_of_range_values() {
        let directory = tempfile::tempdir().unwrap();
        let mut cases = Vec::new();
        let mut config = valid_test_config(&directory);
        config.queue_capacity = MAX_QUEUE_CAPACITY + 1;
        cases.push(config);
        let mut config = valid_test_config(&directory);
        config.concurrency = 2;
        cases.push(config);
        let mut config = valid_test_config(&directory);
        config.replay_timeout = Duration::from_millis(MAX_RUN_TIMEOUT_MS + 1);
        cases.push(config);
        let mut config = valid_test_config(&directory);
        config.suite_timeout = Duration::from_millis(MAX_SUITE_TIMEOUT_MS + 1);
        cases.push(config);
        let mut config = valid_test_config(&directory);
        config.memory_limit_bytes = REQUIRED_MEMORY_LIMIT_BYTES - 1;
        cases.push(config);
        let mut config = valid_test_config(&directory);
        config.frame_limit_bytes = MAX_FRAME_BYTES + 1;
        cases.push(config);
        assert!(cases.into_iter().all(|config| config.validate().is_err()));
    }

    #[test]
    fn admission_enforces_six_per_minute_globally() {
        let mut admission = ProbeAdmissionState::default();
        for id in 20..26 {
            assert_eq!(admission.admit(&request(id).job, 10_000), Ok(()));
        }
        assert_eq!(
            admission.admit(&request(26).job, 10_000),
            Err(ProbeAdmissionError::GlobalRateLimited)
        );
        assert_eq!(admission.admit(&request(26).job, 70_000), Ok(()));
    }

    #[test]
    fn admission_enforces_per_transaction_cooldown_without_consuming_global_budget() {
        let mut admission = ProbeAdmissionState::default();
        let job = request(27).job;
        assert_eq!(admission.admit(&job, 10_000), Ok(()));
        assert_eq!(
            admission.admit(&job, 69_999),
            Err(ProbeAdmissionError::TransactionCooldown)
        );
        assert_eq!(admission.admitted_at.len(), 1);
        assert_eq!(admission.admit(&job, 70_000), Ok(()));
    }

    #[test]
    fn execution_context_requires_exact_height_and_tip_hash() {
        let job = request(28).job;
        assert!(probe_context_matches(&job, 30, &"ab".repeat(32)));
        assert!(!probe_context_matches(&job, 31, &"ab".repeat(32)));
        assert!(!probe_context_matches(&job, 30, &"ac".repeat(32)));
    }

    #[test]
    fn command_policy_clears_environment_and_accepts_no_arguments() {
        let command = restricted_worker_command(Path::new("neo-probe.exe"));
        assert!(WORKER_LAUNCH_POLICY.empty_environment);
        assert_eq!(WORKER_LAUNCH_POLICY.argument_count, 0);
        assert_eq!(command.as_std().get_args().count(), 0);
    }

    #[test]
    fn command_policy_inherits_only_protocol_stdio_handles() {
        assert_eq!(
            WORKER_LAUNCH_POLICY.handle_policy,
            WorkerHandlePolicy::ProtocolStdioOnly
        );
    }

    #[tokio::test]
    async fn changed_context_rejects_before_action_store_admission_or_process_launch() {
        let request = request(29);
        let directory = tempfile::tempdir().unwrap();
        let log = test_log(&directory);
        let launcher = FakeLauncher::new([]);
        let config = valid_test_config(&directory);
        let capabilities = FixedProbeContainment(ProbeContainmentCapability::trusted_for_test(
            config.profile(),
        ));
        let controller = ProbeController::start_with_launcher(
            config,
            log.clone(),
            launcher.clone(),
            Arc::new(FixedContextRechecker {
                height: 31,
                tip_hash: "ab".repeat(32),
            }),
            &capabilities,
        )
        .unwrap();
        let operation_id = request.job.operation_id.clone();
        let result = controller.try_submit(request).unwrap().wait().await;
        assert_eq!(
            result.verdict,
            ProbeVerdict::Uncertain(ProbeUncertainReason::ContextChanged)
        );
        assert!(log.query(&operation_id).unwrap().is_none());
        assert_eq!(launcher.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn queue_full_rejects_without_blocking_producer() {
        let first_request = request(1);
        let bytes = framed(&first_request);
        let (_dir, controller, launcher, _log) = setup(
            1,
            [
                FakeBehavior::Delay(Duration::from_secs(5), bytes.clone()),
                output(bytes),
            ],
            Duration::from_secs(3),
        );
        let first = controller.try_submit(first_request).unwrap();
        launcher.wait_for_calls(1).await;
        let second = controller.try_submit(request(2)).unwrap();
        assert_eq!(
            controller.try_submit(request(3)).unwrap_err(),
            ProbeSubmitError::Full
        );
        first.cancel();
        second.cancel();
    }

    #[tokio::test]
    async fn successful_double_replay_completes_action_store() {
        let request = request(4);
        let bytes = framed(&request);
        let (_dir, controller, launcher, log) = setup(
            16,
            [output(bytes.clone()), output(bytes)],
            Duration::from_secs(1),
        );
        let result = controller.try_submit(request.clone()).unwrap().wait().await;
        assert_eq!(result.operation_id, request.job.operation_id);
        assert_eq!(result.run_id, request.job.run_id);
        assert!(matches!(result.verdict, ProbeVerdict::Completed(_)));
        assert_eq!(launcher.calls.load(Ordering::SeqCst), 2);
        assert_eq!(
            log.query(&result.operation_id).unwrap().unwrap().state,
            crate::action_store::ActionState::Completed
        );
    }

    #[tokio::test]
    async fn complete_replay_mismatch_is_uncertain() {
        let request = request(5);
        let first = framed(&request);
        let mut changed = outcome(&request);
        changed
            .observations
            .insert("different".to_owned(), json!(true));
        let second = encode_frame(&changed).unwrap();
        let (_dir, controller, _, _) =
            setup(16, [output(first), output(second)], Duration::from_secs(1));
        let result = controller.try_submit(request).unwrap().wait().await;
        assert_eq!(
            result.verdict,
            ProbeVerdict::Uncertain(ProbeUncertainReason::ReplayMismatch)
        );
    }

    #[tokio::test]
    async fn replay_timeout_is_uncertain_and_reaped() {
        let request = request(6);
        let bytes = framed(&request);
        let (_dir, controller, launcher, _) = setup(
            16,
            [FakeBehavior::Delay(Duration::from_secs(1), bytes)],
            Duration::from_millis(20),
        );
        let result = controller.try_submit(request).unwrap().wait().await;
        assert_eq!(
            result.verdict,
            ProbeVerdict::Uncertain(ProbeUncertainReason::DeadlineExceeded)
        );
        assert_eq!(launcher.reaped.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn crash_is_uncertain() {
        let request = request(7);
        let operation_id = request.job.operation_id.clone();
        let (_dir, controller, _, log) = setup(
            16,
            [FakeBehavior::Output {
                bytes: Vec::new(),
                stderr: b"worker crashed".to_vec(),
                success: false,
            }],
            Duration::from_secs(1),
        );
        let result = controller.try_submit(request).unwrap().wait().await;
        assert_eq!(
            result.verdict,
            ProbeVerdict::Uncertain(ProbeUncertainReason::ProcessFailed)
        );
        assert_eq!(
            log.query(&operation_id).unwrap().unwrap().state,
            crate::action_store::ActionState::Uncertain
        );
    }

    #[tokio::test]
    async fn oom_indicator_is_uncertain_without_retry() {
        let request = request(15);
        let (_dir, controller, launcher, _) = setup(
            16,
            [FakeBehavior::Output {
                bytes: Vec::new(),
                stderr: b"fatal: out of memory".to_vec(),
                success: false,
            }],
            Duration::from_secs(1),
        );
        let result = controller.try_submit(request).unwrap().wait().await;
        assert_eq!(
            result.verdict,
            ProbeVerdict::Uncertain(ProbeUncertainReason::OutOfMemory)
        );
        assert_eq!(launcher.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn malformed_output_is_uncertain() {
        let request = request(8);
        let (_dir, controller, _, _) = setup(
            16,
            [output(b"not-a-frame".to_vec())],
            Duration::from_secs(1),
        );
        let result = controller.try_submit(request).unwrap().wait().await;
        assert_eq!(
            result.verdict,
            ProbeVerdict::Uncertain(ProbeUncertainReason::Protocol)
        );
    }

    #[tokio::test]
    async fn binding_mismatch_is_uncertain() {
        let request = request(9);
        let mut wrong = outcome(&request);
        wrong.run_id = RunId::new("wrong-run").unwrap();
        let bytes = encode_frame(&wrong).unwrap();
        let (_dir, controller, _, _) = setup(
            16,
            [output(bytes.clone()), output(bytes)],
            Duration::from_secs(1),
        );
        let result = controller.try_submit(request).unwrap().wait().await;
        assert_eq!(
            result.verdict,
            ProbeVerdict::Uncertain(ProbeUncertainReason::BindingMismatch)
        );
    }

    #[tokio::test]
    async fn cancellation_and_controller_drop_reap_active_processes() {
        let first_request = request(10);
        let first_bytes = framed(&first_request);
        let (_dir, controller, launcher, _) = setup(
            16,
            [FakeBehavior::Delay(Duration::from_secs(5), first_bytes)],
            Duration::from_secs(3),
        );
        let submission = controller.try_submit(first_request).unwrap();
        launcher.wait_for_calls(1).await;
        submission.cancel();
        let result = submission.wait().await;
        assert_eq!(
            result.verdict,
            ProbeVerdict::Uncertain(ProbeUncertainReason::Cancelled)
        );
        assert_eq!(launcher.reaped.load(Ordering::SeqCst), 1);

        let second_request = request(11);
        let second_bytes = framed(&second_request);
        let (_dir, controller, launcher, _) = setup(
            16,
            [FakeBehavior::Delay(Duration::from_secs(5), second_bytes)],
            Duration::from_secs(3),
        );
        let submission = controller.try_submit(second_request).unwrap();
        launcher.wait_for_calls(1).await;
        drop(controller);
        let result = submission.wait().await;
        assert_eq!(
            result.verdict,
            ProbeVerdict::Uncertain(ProbeUncertainReason::Cancelled)
        );
        assert_eq!(launcher.reaped.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn worker_concurrency_is_exactly_one() {
        let first = request(12);
        let second = request(13);
        let first_bytes = framed(&first);
        let second_bytes = framed(&second);
        let (_dir, controller, launcher, _) = setup(
            16,
            [
                FakeBehavior::Delay(Duration::from_millis(30), first_bytes.clone()),
                output(first_bytes),
                output(second_bytes.clone()),
                output(second_bytes),
            ],
            Duration::from_secs(1),
        );
        let first = controller.try_submit(first).unwrap();
        let second = controller.try_submit(second).unwrap();
        assert!(matches!(
            first.wait().await.verdict,
            ProbeVerdict::Completed(_)
        ));
        assert!(matches!(
            second.wait().await.verdict,
            ProbeVerdict::Completed(_)
        ));
        assert_eq!(launcher.max_active.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn failed_process_is_never_automatically_retried() {
        let request = request(14);
        let bytes = framed(&request);
        let (_dir, controller, launcher, _) = setup(
            16,
            [
                FakeBehavior::Output {
                    bytes: Vec::new(),
                    stderr: Vec::new(),
                    success: false,
                },
                output(bytes),
            ],
            Duration::from_secs(1),
        );
        let result = controller.try_submit(request).unwrap().wait().await;
        assert!(matches!(result.verdict, ProbeVerdict::Uncertain(_)));
        assert_eq!(launcher.calls.load(Ordering::SeqCst), 1);
    }
}
