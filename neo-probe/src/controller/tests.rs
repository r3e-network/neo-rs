use super::runner::{
    LaunchError, ProcessOutput, RunCancellation, WORKER_LAUNCH_POLICY, WorkerHandlePolicy,
    restricted_worker_command,
};
#[cfg(windows)]
use super::runner::{WindowsJob, set_job_extended_limits};
use super::*;
use crate::containment::{ContainmentProperty, FixedProbeContainment, ProbeContainmentEvidence};
use crate::{SnapshotEntry, SnapshotInput};
use neo_ai_agent::{
    AuthorizerMetadata, EvidenceAuthority, ProbeAction, ProbeBudget, ProbeBuild, ProbeContext,
    ProbeTransaction,
};
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
        _executable: &Path,
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
        let behavior = self
            .behaviors
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
    hex::encode(Sha256::digest(bytes).as_slice())
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
            version: "neo-probe-0.21.0".to_owned(),
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
            // StorageKey-encoded key (i32-LE service id 29 + 3-byte
            // suffix), mirroring `StorageKey::new(29, vec![1, 2,
            // 3]).to_array()`: the real worker's `DetachedSnapshot`
            // rejects raw keys shorter than 4 bytes.
            key: vec![29, 0, 0, 0, 1, 2, 3],
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
    let log = Arc::new(DurableActionStore::open(directory.path().join("actions.jsonl")).unwrap());
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
    let capabilities = FixedProbeContainment(ProbeContainmentCapability::trusted_for_test(wrong));
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
        neo_ai_agent::ActionState::Completed
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
        neo_ai_agent::ActionState::Uncertain
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

// ---------------------------------------------------------------------
// Task 44: real Windows Job Object containment. These tests exercise the
// actual platform boundary instead of the injected `FakeLauncher`.
// ---------------------------------------------------------------------

#[cfg(windows)]
fn cmd_exe() -> PathBuf {
    let system_root = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    PathBuf::from(system_root).join("System32").join("cmd.exe")
}

#[cfg(windows)]
#[test]
fn windows_job_configures_exact_limit_inventory() {
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
        JOB_OBJECT_LIMIT_JOB_TIME, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOB_OBJECT_LIMIT_PROCESS_TIME,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        QueryInformationJobObject,
    };

    let job = WindowsJob::create(
        REQUIRED_MEMORY_LIMIT_BYTES,
        Duration::from_millis(MAX_RUN_TIMEOUT_MS),
    )
    .expect("job object with the restricted limit inventory");
    let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    let mut returned = 0u32;
    // SAFETY: `info` is initialized for the queried information class and
    // `job.0` is the live owned job handle.
    let queried = unsafe {
        QueryInformationJobObject(
            job.0,
            JobObjectExtendedLimitInformation,
            std::ptr::from_mut(&mut info).cast(),
            u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                .unwrap_or(u32::MAX),
            &mut returned,
        )
    };
    assert_eq!(queried, 1, "querying the applied limits must succeed");
    let flags = info.BasicLimitInformation.LimitFlags;
    assert!(flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE != 0);
    assert!(flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS != 0);
    assert!(flags & JOB_OBJECT_LIMIT_PROCESS_MEMORY != 0);
    assert!(flags & JOB_OBJECT_LIMIT_PROCESS_TIME != 0);
    assert!(flags & JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION != 0);
    assert_eq!(flags & JOB_OBJECT_LIMIT_JOB_TIME, 0);
    assert_eq!(info.BasicLimitInformation.ActiveProcessLimit, 1);
    assert_eq!(
        info.ProcessMemoryLimit,
        REQUIRED_MEMORY_LIMIT_BYTES as usize
    );
    assert_eq!(
        info.BasicLimitInformation.PerProcessUserTimeLimit,
        i64::try_from(MAX_RUN_TIMEOUT_MS * 10_000).unwrap()
    );
}

#[cfg(windows)]
#[test]
fn windows_job_failure_paths_reject_without_panic() {
    // An invalid job handle must be rejected by the limit configuration,
    // never panic and never degrade into an uncontained launch.
    assert!(
        set_job_extended_limits(
            std::ptr::null_mut(),
            REQUIRED_MEMORY_LIMIT_BYTES,
            Duration::from_millis(MAX_RUN_TIMEOUT_MS),
        )
        .is_err()
    );

    let job = WindowsJob::create(
        REQUIRED_MEMORY_LIMIT_BYTES,
        Duration::from_millis(MAX_RUN_TIMEOUT_MS),
    )
    .expect("live job object");
    // An invalid process handle must be rejected by assignment.
    assert!(job.assign_process(std::ptr::null_mut()).is_err());
    // The failure did not poison the job: its limits stay configurable.
    assert!(
        job.configure(
            REQUIRED_MEMORY_LIMIT_BYTES,
            Duration::from_millis(MAX_RUN_TIMEOUT_MS)
        )
        .is_ok()
    );
}

#[cfg(windows)]
#[test]
fn windows_job_governs_helper_process_communication_and_exit() {
    use std::os::windows::io::AsRawHandle;

    let job = WindowsJob::create(
        REQUIRED_MEMORY_LIMIT_BYTES,
        Duration::from_millis(MAX_RUN_TIMEOUT_MS),
    )
    .expect("job object with the restricted limit inventory");
    // Harmless smoke helper: `cmd.exe /c ver` prints the Windows version
    // banner to stdout and exits 0 without touching anything else.
    let child = std::process::Command::new(cmd_exe())
        .args(["/c", "ver"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("harmless helper process");
    job.assign_process(child.as_raw_handle().cast())
        .expect("helper must be governed by the job before it runs");
    let output = child.wait_with_output().expect("helper under job");
    assert!(
        output.status.success(),
        "governed helper must run to completion: {}",
        output.status
    );
    assert!(
        !output.stdout.is_empty(),
        "governed helper must communicate over the stdout pipe"
    );
    // Dropping the job handle after a clean exit must not resurrect or
    // leak anything; the child was already reaped by `wait_with_output`.
    drop(job);
}

#[cfg(windows)]
#[test]
fn windows_job_close_terminates_governed_process_without_orphan() {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::JobObjects::{
        JOBOBJECT_BASIC_PROCESS_ID_LIST, JobObjectBasicProcessIdList, QueryInformationJobObject,
    };

    // Interactive `cmd.exe` with a held-open stdin pipe stays alive until
    // the job handle is dropped (the kill-on-close boundary).
    let mut child = std::process::Command::new(cmd_exe())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("harmless helper process");
    let job = WindowsJob::create(
        REQUIRED_MEMORY_LIMIT_BYTES,
        Duration::from_millis(MAX_RUN_TIMEOUT_MS),
    )
    .expect("job object with the restricted limit inventory");
    job.assign_process(child.as_raw_handle().cast())
        .expect("helper must be governed by the job");
    // Prove the process really lives inside the job.
    let mut member_list = JOBOBJECT_BASIC_PROCESS_ID_LIST::default();
    let mut returned = 0u32;
    // SAFETY: `member_list` is initialized for the queried class and the
    // job holds exactly one member.
    let listed = unsafe {
        QueryInformationJobObject(
            job.0,
            JobObjectBasicProcessIdList,
            std::ptr::from_mut(&mut member_list).cast(),
            u32::try_from(std::mem::size_of::<JOBOBJECT_BASIC_PROCESS_ID_LIST>())
                .unwrap_or(u32::MAX),
            &mut returned,
        )
    };
    assert_eq!(listed, 1, "job member query must succeed");
    assert_eq!(member_list.NumberOfProcessIdsInList, 1);
    assert_eq!(member_list.ProcessIdList[0], child.id() as usize);
    // Hold stdin open so the helper stays alive until job close.
    let _held_stdin = child.stdin.take().expect("held stdin pipe");
    assert!(
        child.try_wait().expect("try_wait").is_none(),
        "governed helper must be alive before the job handle is dropped"
    );
    drop(job);
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().expect("try_wait after job close") {
            // The helper was proven alive with its stdin pipe still held
            // open right before the only intervening action - closing the
            // job handle - so any exit here is the kill-on-close boundary
            // doing the reaping. The exit code Windows assigns to a
            // job-terminated process is not contractual, so it is not
            // asserted.
            assert!(
                status.code().is_some(),
                "job close must reap the governed process without an explicit kill"
            );
            return;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "job close must reap the governed process promptly"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn native_startup_gate_stays_closed_without_complete_isolation() {
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join(if cfg!(windows) {
        "neo-probe.exe"
    } else {
        "neo-probe"
    });
    std::fs::write(&executable, b"worker test fixture").unwrap();
    let mut section = DiagnosticsSection::default();
    section.restricted_probe_enabled = true;
    section.restricted_probe_worker_path = executable.to_string_lossy().into_owned();
    assert!(matches!(
        validate_probe_startup(&section, &NativeProbeContainment),
        Err(ProbeStartupError::UnsupportedContainment(_))
    ));
}
