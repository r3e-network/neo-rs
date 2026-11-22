use super::*;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeAdmissionError {
    GlobalRateLimited,
    TransactionCooldown,
}
#[derive(Debug, Default)]
pub(super) struct ProbeAdmissionState {
    pub(super) admitted_at: VecDeque<u64>,
    pub(super) transaction_last_admitted: HashMap<String, u64>,
}

impl ProbeAdmissionState {
    pub(super) fn admit(
        &mut self,
        job: &ProbeJob,
        now_unix_ms: u64,
    ) -> Result<(), ProbeAdmissionError> {
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
pub trait ProbeContextRechecker: Send + Sync {
    fn is_current(&self, job: &ProbeJob) -> bool;
}
pub fn probe_context_matches(job: &ProbeJob, current_height: u32, current_tip_hash: &str) -> bool {
    job.context.observed_height == current_height && job.context.tip_hash == current_tip_hash
}
#[derive(Debug, Clone)]
pub struct ProbeControllerConfig {
    pub executable: PathBuf,
    pub queue_capacity: usize,
    pub concurrency: usize,
    pub replay_timeout: Duration,
    pub suite_timeout: Duration,
    pub memory_limit_bytes: u64,
    pub frame_limit_bytes: usize,
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
    pub(super) fn profile(&self) -> ProbeContainmentProfile {
        ProbeContainmentProfile {
            name: crate::containment::RESTRICTED_PROBE_PROFILE,
            memory_limit_bytes: self.memory_limit_bytes,
            run_timeout_ms: u64::try_from(self.replay_timeout.as_millis()).unwrap_or(u64::MAX),
            suite_timeout_ms: u64::try_from(self.suite_timeout.as_millis()).unwrap_or(u64::MAX),
        }
    }

    pub(super) fn validate(mut self) -> Result<Self, ProbeStartupError> {
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
pub enum ProbeStartupError {
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
        || raw.contains('\0')
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
pub fn validate_probe_startup(
    section: &DiagnosticsSection,
    capabilities: &dyn ProbeContainmentCapabilityProvider,
) -> Result<ProbeControllerConfig, ProbeStartupError> {
    validate_config_with_capability(ProbeControllerConfig::from(section), capabilities)
}
pub(super) fn validate_config_with_capability(
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
