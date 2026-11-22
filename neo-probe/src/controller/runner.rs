use super::*;
#[derive(Debug, Clone)]
pub(super) struct RunCancellation {
    pub(super) controller: CancellationToken,
    pub(super) job: CancellationToken,
}
#[derive(Debug)]
pub(super) struct ProcessOutput {
    pub(super) success: bool,
    pub(super) stdout: Vec<u8>,
    pub(super) stderr: Vec<u8>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LaunchError {
    Cancelled,
    Deadline,
    Spawn,
    Containment,
    Io,
    OutputTooLarge,
}
impl LaunchError {
    pub(super) const fn reason(self) -> ProbeUncertainReason {
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
pub(super) trait ProcessLauncher: Send + Sync {
    async fn run(
        &self,
        executable: &Path,
        stdin: Vec<u8>,
        replay_timeout: Duration,
        suite_deadline: Instant,
        memory_limit_bytes: u64,
        frame_limit_bytes: usize,
        cancellation: RunCancellation,
    ) -> Result<ProcessOutput, LaunchError>;
}
#[derive(Debug, Clone, Copy)]
pub(super) struct CommandProcessLauncher;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkerHandlePolicy {
    ProtocolStdioOnly,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WorkerLaunchPolicy {
    pub(super) empty_environment: bool,
    pub(super) handle_policy: WorkerHandlePolicy,
    pub(super) argument_count: usize,
}
pub(super) const WORKER_LAUNCH_POLICY: WorkerLaunchPolicy = WorkerLaunchPolicy {
    empty_environment: true,
    handle_policy: WorkerHandlePolicy::ProtocolStdioOnly,
    argument_count: 0,
};
pub(super) fn restricted_worker_command(executable: &Path) -> Command {
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
pub(super) struct WindowsJob(pub(super) windows_sys::Win32::Foundation::HANDLE);
#[cfg(windows)]
// SAFETY: The owned Job Object handle may be closed from any thread.
unsafe impl Send for WindowsJob {}
#[cfg(windows)]
impl WindowsJob {
    /// Creates a private, non-inheritable Job Object and applies the fixed
    /// containment limit inventory before any process is assigned to it.
    pub(super) fn create(
        memory_limit_bytes: u64,
        replay_timeout: Duration,
    ) -> std::io::Result<Self> {
        use windows_sys::Win32::System::JobObjects::CreateJobObjectW;

        // SAFETY: Null security attributes and name request a private,
        // non-inheritable job; breakaway is never granted.
        let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if handle.is_null() {
            return Err(std::io::Error::last_os_error());
        }
        let job = Self(handle);
        job.configure(memory_limit_bytes, replay_timeout)?;
        Ok(job)
    }

    /// Applies the exact extended-limit inventory. Every Job API failure is a
    /// rejection: the caller must refuse to launch rather than degrade.
    pub(super) fn configure(
        &self,
        memory_limit_bytes: u64,
        replay_timeout: Duration,
    ) -> std::io::Result<()> {
        set_job_extended_limits(self.0, memory_limit_bytes, replay_timeout)
    }

    /// Assigns a live tokio child to this job.
    fn assign(&self, child: &tokio::process::Child) -> std::io::Result<()> {
        let process = child.raw_handle().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "worker exited before job assignment",
            )
        })?;
        self.assign_process(process.cast())
    }

    /// Assigns a raw process handle to this job.
    ///
    /// The child is blocked reading the frame header from its stdin pipe at
    /// this point, so the tiny spawn-to-assignment window cannot be used for
    /// work before the job governs the process.
    pub(super) fn assign_process(
        &self,
        process: windows_sys::Win32::Foundation::HANDLE,
    ) -> std::io::Result<()> {
        use windows_sys::Win32::System::JobObjects::AssignProcessToJobObject;

        // SAFETY: `self.0` is the live owned job handle and `process` is a live
        // process handle owned by the caller.
        let assigned = unsafe { AssignProcessToJobObject(self.0, process) };
        if assigned == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    }
}
/// Applies the exact Job Object limit inventory for one restricted worker run.
///
/// * `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` - the launcher holds the only job
///   handle, so dropping it terminates and reaps the whole worker tree,
///   aligned with the launcher's `kill_on_drop` semantics.
/// * `JOB_OBJECT_LIMIT_ACTIVE_PROCESS = 1` - no second process can live in
///   the job, denying sideways process escape.
/// * `JOB_OBJECT_LIMIT_PROCESS_MEMORY` - exact configured commit ceiling.
/// * `JOB_OBJECT_LIMIT_PROCESS_TIME` - per-process user-time ceiling derived
///   from the replay budget; CPU rate control is deliberately not configured
///   because it only shapes scheduling and cannot stop one fast core.
/// * `JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION` - a crashing worker dies
///   instead of surviving as an orphan.
#[cfg(windows)]
pub(super) fn set_job_extended_limits(
    handle: windows_sys::Win32::Foundation::HANDLE,
    memory_limit_bytes: u64,
    replay_timeout: Duration,
) -> std::io::Result<()> {
    use std::mem::size_of;
    use windows_sys::Win32::System::JobObjects::{
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS, JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOB_OBJECT_LIMIT_PROCESS_MEMORY,
        JOB_OBJECT_LIMIT_PROCESS_TIME, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject,
    };

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
            handle,
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>()).unwrap_or(u32::MAX),
        )
    };
    if configured == 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
#[cfg(windows)]
impl Drop for WindowsJob {
    fn drop(&mut self) {
        // SAFETY: This type uniquely owns the Job Object handle; closing it is
        // the kill-on-close containment boundary for any worker stragglers.
        unsafe {
            windows_sys::Win32::Foundation::CloseHandle(self.0);
        }
    }
}
#[async_trait]
impl ProcessLauncher for CommandProcessLauncher {
    async fn run(
        &self,
        executable: &Path,
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
        // Fail closed: create the governed job first, assign the freshly
        // spawned worker, and treat every Job API failure as a launch
        // rejection with the already-spawned child reaped below - the worker
        // must never keep running outside a fully configured job.
        let Ok(_job) = WindowsJob::create(memory_limit_bytes, replay_timeout)
            .and_then(|job| job.assign(&child).map(|()| job))
        else {
            terminate_and_reap(&mut child).await;
            return Err(LaunchError::Containment);
        };
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
