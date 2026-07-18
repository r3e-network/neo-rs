use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::HashSet;
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

/// Linux process I/O counters from `/proc/self/io`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct ProcessIoSnapshot {
    /// Bytes passed to read syscalls, including cache hits.
    pub read_chars: u64,
    /// Bytes passed to write syscalls, including buffered writes.
    pub write_chars: u64,
    /// Read-like syscall count.
    pub read_syscalls: u64,
    /// Write-like syscall count.
    pub write_syscalls: u64,
    /// Bytes fetched from the storage layer for this process.
    pub read_bytes: u64,
    /// Bytes submitted to the storage layer for this process.
    pub write_bytes: u64,
    /// Write bytes cancelled before reaching storage. Kept separate from
    /// `write_bytes`; reports never silently subtract it.
    pub cancelled_write_bytes: u64,
}

/// Point-in-time process CPU, RSS, and I/O evidence.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProcessSnapshot {
    /// Whether Linux procfs counters were available.
    pub available: bool,
    /// User CPU clock ticks from `/proc/self/stat`.
    pub user_cpu_ticks: Option<u64>,
    /// System CPU clock ticks from `/proc/self/stat`.
    pub system_cpu_ticks: Option<u64>,
    /// Process resident bytes at this boundary.
    pub rss_bytes: Option<u64>,
    /// Process-lifetime peak resident bytes.
    pub rss_high_water_bytes: Option<u64>,
    /// Anonymous resident bytes.
    pub rss_anon_bytes: Option<u64>,
    /// File-backed resident bytes.
    pub rss_file_bytes: Option<u64>,
    /// Shared-memory resident bytes.
    pub rss_shared_bytes: Option<u64>,
    /// Process-attributed I/O counters.
    pub io: Option<ProcessIoSnapshot>,
}

/// Recursive retained-size evidence for one benchmark database directory.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FileTreeSnapshot {
    /// Regular files, deduplicated by device and inode on Unix.
    pub regular_files: u64,
    /// Directories traversed without following symlinks.
    pub directories: u64,
    /// Symlinks observed and intentionally not followed.
    pub symlinks: u64,
    /// Sum of regular-file logical lengths.
    pub logical_bytes: u64,
    /// Sum of retained allocated blocks (`st_blocks * 512`) on Unix.
    /// This is footprint, not write traffic.
    pub allocated_bytes: Option<u64>,
}

/// Correlated process and database evidence at one phase boundary.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct EvidenceSnapshot {
    /// Wall-clock Unix timestamp in nanoseconds.
    pub unix_time_ns: u128,
    /// Process counters.
    pub process: ProcessSnapshot,
    /// Database tree counters.
    pub files: FileTreeSnapshot,
}

/// Signed deltas between two process snapshots.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct ProcessDelta {
    /// User CPU ticks consumed.
    pub user_cpu_ticks: Option<u64>,
    /// System CPU ticks consumed.
    pub system_cpu_ticks: Option<u64>,
    /// User plus system CPU seconds when `AT_CLKTCK` was available.
    pub cpu_seconds: Option<f64>,
    /// CPU utilization relative to one fully occupied core.
    pub one_core_utilization_percent: Option<f64>,
    /// Change in process-attributed storage reads.
    pub read_bytes: Option<u64>,
    /// Change in process-attributed storage writes.
    pub write_bytes: Option<u64>,
    /// Change in cancelled write bytes, reported independently.
    pub cancelled_write_bytes: Option<u64>,
    /// Change in write characters.
    pub write_chars: Option<u64>,
    /// Change in write-like syscall count.
    pub write_syscalls: Option<u64>,
}

/// Signed retained-size changes between file-tree snapshots.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct FileTreeDelta {
    /// Logical regular-file growth, which may be negative after MDBX resize.
    pub logical_bytes: i64,
    /// Allocated-byte growth on Unix.
    pub allocated_bytes: Option<i64>,
    /// Change in deduplicated regular-file count.
    pub regular_files: i64,
}

/// Full evidence delta for one measured phase.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct EvidenceDelta {
    /// Process evidence delta.
    pub process: ProcessDelta,
    /// File-tree evidence delta.
    pub files: FileTreeDelta,
}

/// Peak RSS observed by the bounded sampler during one measured phase.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct SampledRssPeak {
    /// Maximum sampled `VmRSS` in bytes.
    pub peak_rss_bytes: Option<u64>,
    /// Number of successful samples.
    pub samples: u64,
    /// Requested sampling interval.
    pub interval_ms: u64,
}

#[derive(Debug, Serialize)]
struct EvidenceEvent<'a> {
    schema_version: u32,
    pid: u32,
    phase: &'a str,
    boundary: &'a str,
    elapsed_ns: u128,
    snapshot: &'a EvidenceSnapshot,
}

/// Optional JSONL checkpoint stream for correlating external device samplers.
pub struct EvidenceLog {
    writer: Option<BufWriter<File>>,
    started: Instant,
}

impl EvidenceLog {
    /// Creates or truncates the requested evidence log.
    pub fn new(path: Option<&Path>) -> Result<Self> {
        let writer = path
            .map(|path| {
                if let Some(parent) = path
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                {
                    fs::create_dir_all(parent).with_context(|| {
                        format!("create evidence-log directory {}", parent.display())
                    })?;
                }
                File::create(path)
                    .map(BufWriter::new)
                    .with_context(|| format!("create evidence log {}", path.display()))
            })
            .transpose()?;
        Ok(Self {
            writer,
            started: Instant::now(),
        })
    }

    /// Emits one phase boundary to stderr and, when configured, JSONL.
    pub fn checkpoint(
        &mut self,
        phase: &str,
        boundary: &str,
        snapshot: &EvidenceSnapshot,
    ) -> Result<()> {
        let elapsed_ns = self.started.elapsed().as_nanos();
        eprintln!(
            "mdbx-bench-evidence pid={} phase={phase} boundary={boundary} elapsed_ns={elapsed_ns}",
            std::process::id()
        );
        if let Some(writer) = self.writer.as_mut() {
            let event = EvidenceEvent {
                schema_version: 1,
                pid: std::process::id(),
                phase,
                boundary,
                elapsed_ns,
                snapshot,
            };
            serde_json::to_writer(&mut *writer, &event).context("encode evidence checkpoint")?;
            writer
                .write_all(b"\n")
                .context("terminate evidence checkpoint")?;
            writer.flush().context("flush evidence checkpoint")?;
        }
        Ok(())
    }
}

/// Samples current RSS every 50 ms without retaining unbounded history.
pub struct RssSampler {
    stop: Arc<AtomicBool>,
    peak: Arc<AtomicU64>,
    samples: Arc<AtomicU64>,
    worker: Option<JoinHandle<()>>,
    interval: Duration,
}

impl RssSampler {
    /// Starts a bounded RSS sampler.
    pub fn start(interval: Duration) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let peak = Arc::new(AtomicU64::new(0));
        let samples = Arc::new(AtomicU64::new(0));
        let worker_stop = Arc::clone(&stop);
        let worker_peak = Arc::clone(&peak);
        let worker_samples = Arc::clone(&samples);
        let worker = thread::spawn(move || {
            loop {
                if let Some(rss) = read_process_status().and_then(|status| status.rss_bytes) {
                    worker_peak.fetch_max(rss, Ordering::Relaxed);
                    worker_samples.fetch_add(1, Ordering::Relaxed);
                }
                if worker_stop.load(Ordering::Relaxed) {
                    break;
                }
                thread::park_timeout(interval);
            }
        });
        Self {
            stop,
            peak,
            samples,
            worker: Some(worker),
            interval,
        }
    }

    /// Stops the sampler and returns its bounded summary.
    pub fn finish(mut self) -> SampledRssPeak {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
        let samples = self.samples.load(Ordering::Relaxed);
        SampledRssPeak {
            peak_rss_bytes: (samples > 0).then(|| self.peak.load(Ordering::Relaxed)),
            samples,
            interval_ms: self.interval.as_millis().try_into().unwrap_or(u64::MAX),
        }
    }
}

impl Drop for RssSampler {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(worker) = self.worker.take() {
            worker.thread().unpark();
            let _ = worker.join();
        }
    }
}

/// Captures process and recursive database evidence.
pub fn capture_evidence(database: &Path) -> Result<EvidenceSnapshot> {
    Ok(EvidenceSnapshot {
        unix_time_ns: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos(),
        process: capture_process(),
        files: capture_file_tree(database)?,
    })
}

/// Computes a phase delta without conflating footprint with write traffic.
pub fn evidence_delta(
    before: &EvidenceSnapshot,
    after: &EvidenceSnapshot,
    wall: Duration,
    clock_ticks_per_second: Option<u64>,
) -> EvidenceDelta {
    EvidenceDelta {
        process: process_delta(
            &before.process,
            &after.process,
            wall,
            clock_ticks_per_second,
        ),
        files: FileTreeDelta {
            logical_bytes: signed_delta(before.files.logical_bytes, after.files.logical_bytes),
            allocated_bytes: before
                .files
                .allocated_bytes
                .zip(after.files.allocated_bytes)
                .map(|(before, after)| signed_delta(before, after)),
            regular_files: signed_delta(before.files.regular_files, after.files.regular_files),
        },
    }
}

/// Reads the kernel clock-tick frequency from Linux auxiliary vectors.
pub fn clock_ticks_per_second() -> Option<u64> {
    let bytes = fs::read("/proc/self/auxv").ok()?;
    parse_auxv_clock_ticks(&bytes)
}

fn parse_auxv_clock_ticks(bytes: &[u8]) -> Option<u64> {
    let word = std::mem::size_of::<usize>();
    for entry in bytes.chunks_exact(word * 2) {
        let key = native_word(&entry[..word])?;
        let value = native_word(&entry[word..])?;
        if key == 17 {
            return u64::try_from(value).ok().filter(|value| *value > 0);
        }
        if key == 0 {
            break;
        }
    }
    None
}

fn capture_process() -> ProcessSnapshot {
    let cpu = fs::read_to_string("/proc/self/stat")
        .ok()
        .and_then(|text| parse_process_stat(&text));
    let status = read_process_status();
    let io = fs::read_to_string("/proc/self/io")
        .ok()
        .and_then(|text| parse_process_io(&text));
    ProcessSnapshot {
        available: cpu.is_some() || status.is_some() || io.is_some(),
        user_cpu_ticks: cpu.map(|cpu| cpu.0),
        system_cpu_ticks: cpu.map(|cpu| cpu.1),
        rss_bytes: status.and_then(|status| status.rss_bytes),
        rss_high_water_bytes: status.and_then(|status| status.rss_high_water_bytes),
        rss_anon_bytes: status.and_then(|status| status.rss_anon_bytes),
        rss_file_bytes: status.and_then(|status| status.rss_file_bytes),
        rss_shared_bytes: status.and_then(|status| status.rss_shared_bytes),
        io,
    }
}

fn capture_file_tree(root: &Path) -> Result<FileTreeSnapshot> {
    if !root.exists() {
        return Ok(FileTreeSnapshot::default());
    }
    let mut result = FileTreeSnapshot::default();
    let mut pending = vec![PathBuf::from(root)];
    #[cfg(unix)]
    let mut inodes = HashSet::new();
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("read file evidence metadata {}", path.display()))?;
        if metadata.file_type().is_symlink() {
            result.symlinks = result
                .symlinks
                .checked_add(1)
                .context("file evidence symlink count overflows u64")?;
        } else if metadata.is_dir() {
            result.directories = result
                .directories
                .checked_add(1)
                .context("file evidence directory count overflows u64")?;
            for entry in fs::read_dir(&path)
                .with_context(|| format!("read evidence directory {}", path.display()))?
            {
                pending.push(entry.context("read evidence directory entry")?.path());
            }
        } else if metadata.is_file() {
            #[cfg(unix)]
            if !inodes.insert((metadata.dev(), metadata.ino())) {
                continue;
            }
            result.regular_files = result
                .regular_files
                .checked_add(1)
                .context("file evidence regular-file count overflows u64")?;
            result.logical_bytes = result
                .logical_bytes
                .checked_add(metadata.len())
                .context("file evidence logical byte count overflows u64")?;
            #[cfg(unix)]
            {
                let allocated = metadata
                    .blocks()
                    .checked_mul(512)
                    .context("file evidence allocated byte count overflows u64")?;
                result.allocated_bytes = Some(
                    result
                        .allocated_bytes
                        .unwrap_or_default()
                        .checked_add(allocated)
                        .context("file evidence allocated byte sum overflows u64")?,
                );
            }
        }
    }
    Ok(result)
}

fn process_delta(
    before: &ProcessSnapshot,
    after: &ProcessSnapshot,
    wall: Duration,
    clock_ticks_per_second: Option<u64>,
) -> ProcessDelta {
    let user = option_delta(before.user_cpu_ticks, after.user_cpu_ticks);
    let system = option_delta(before.system_cpu_ticks, after.system_cpu_ticks);
    let total_ticks = user
        .zip(system)
        .and_then(|(user, system)| user.checked_add(system));
    let cpu_seconds = total_ticks
        .zip(clock_ticks_per_second)
        .and_then(|(ticks, hz)| (hz > 0).then(|| ticks as f64 / hz as f64));
    let one_core_utilization_percent = cpu_seconds.and_then(|seconds| {
        (wall.as_secs_f64() > 0.0).then(|| seconds / wall.as_secs_f64() * 100.0)
    });
    let io = before.io.zip(after.io);
    ProcessDelta {
        user_cpu_ticks: user,
        system_cpu_ticks: system,
        cpu_seconds,
        one_core_utilization_percent,
        read_bytes: io.and_then(|(before, after)| after.read_bytes.checked_sub(before.read_bytes)),
        write_bytes: io
            .and_then(|(before, after)| after.write_bytes.checked_sub(before.write_bytes)),
        cancelled_write_bytes: io.and_then(|(before, after)| {
            after
                .cancelled_write_bytes
                .checked_sub(before.cancelled_write_bytes)
        }),
        write_chars: io
            .and_then(|(before, after)| after.write_chars.checked_sub(before.write_chars)),
        write_syscalls: io
            .and_then(|(before, after)| after.write_syscalls.checked_sub(before.write_syscalls)),
    }
}

fn option_delta(before: Option<u64>, after: Option<u64>) -> Option<u64> {
    before
        .zip(after)
        .and_then(|(before, after)| after.checked_sub(before))
}

fn signed_delta(before: u64, after: u64) -> i64 {
    let delta = i128::from(after) - i128::from(before);
    delta.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

#[derive(Clone, Copy)]
struct ProcessStatus {
    rss_bytes: Option<u64>,
    rss_high_water_bytes: Option<u64>,
    rss_anon_bytes: Option<u64>,
    rss_file_bytes: Option<u64>,
    rss_shared_bytes: Option<u64>,
}

fn read_process_status() -> Option<ProcessStatus> {
    fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|text| parse_process_status(&text))
}

fn parse_process_stat(text: &str) -> Option<(u64, u64)> {
    let close = text.rfind(')')?;
    let fields = text
        .get(close + 1..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    Some((fields.get(11)?.parse().ok()?, fields.get(12)?.parse().ok()?))
}

fn parse_process_io(text: &str) -> Option<ProcessIoSnapshot> {
    let field = |name: &str| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            (key.trim() == name).then(|| value.trim().parse::<u64>().ok())?
        })
    };
    Some(ProcessIoSnapshot {
        read_chars: field("rchar")?,
        write_chars: field("wchar")?,
        read_syscalls: field("syscr")?,
        write_syscalls: field("syscw")?,
        read_bytes: field("read_bytes")?,
        write_bytes: field("write_bytes")?,
        cancelled_write_bytes: field("cancelled_write_bytes")?,
    })
}

fn parse_process_status(text: &str) -> Option<ProcessStatus> {
    let kib = |name: &str| {
        text.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            if key != name {
                return None;
            }
            value
                .split_whitespace()
                .next()?
                .parse::<u64>()
                .ok()
                .and_then(|value| value.checked_mul(1024))
        })
    };
    let status = ProcessStatus {
        rss_bytes: kib("VmRSS"),
        rss_high_water_bytes: kib("VmHWM"),
        rss_anon_bytes: kib("RssAnon"),
        rss_file_bytes: kib("RssFile"),
        rss_shared_bytes: kib("RssShmem"),
    };
    status.rss_bytes.map(|_| status)
}

fn native_word(bytes: &[u8]) -> Option<usize> {
    match bytes.len() {
        8 => usize::try_from(u64::from_ne_bytes(bytes.try_into().ok()?)).ok(),
        4 => usize::try_from(u32::from_ne_bytes(bytes.try_into().ok()?)).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn proc_stat_parser_handles_spaces_and_parentheses_in_comm() {
        let mut fields = vec!["S".to_string()];
        fields.extend((4..=13).map(|value| value.to_string()));
        fields.push("140".to_string());
        fields.push("150".to_string());
        fields.extend((16..=30).map(|value| value.to_string()));
        let text = format!("7 (bench worker (1)) {}", fields.join(" "));
        assert_eq!(parse_process_stat(&text), Some((140, 150)));
    }

    #[test]
    fn proc_io_and_status_parsers_preserve_physical_write_semantics() {
        let io = parse_process_io(
            "rchar: 10\nwchar: 20\nsyscr: 3\nsyscw: 4\nread_bytes: 4096\nwrite_bytes: 8192\ncancelled_write_bytes: 1024\n",
        )
        .expect("parse process io");
        assert_eq!(io.write_bytes, 8192);
        assert_eq!(io.cancelled_write_bytes, 1024);

        let status = parse_process_status(
            "VmHWM:\t200 kB\nVmRSS:\t100 kB\nRssAnon:\t60 kB\nRssFile:\t30 kB\nRssShmem:\t10 kB\n",
        )
        .expect("parse status");
        assert_eq!(status.rss_bytes, Some(102_400));
        assert_eq!(status.rss_file_bytes, Some(30_720));
    }

    #[test]
    fn auxv_parser_reads_clock_ticks_and_honors_the_null_terminator() {
        let mut bytes = Vec::new();
        for (key, value) in [(3usize, 4096usize), (17, 250), (0, 0), (17, 999)] {
            bytes.extend_from_slice(&key.to_ne_bytes());
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
        assert_eq!(parse_auxv_clock_ticks(&bytes), Some(250));

        let mut terminated = Vec::new();
        for (key, value) in [(3usize, 4096usize), (0, 0), (17, 999)] {
            terminated.extend_from_slice(&key.to_ne_bytes());
            terminated.extend_from_slice(&value.to_ne_bytes());
        }
        assert_eq!(parse_auxv_clock_ticks(&terminated), None);
    }

    #[test]
    fn regressing_process_counters_are_unavailable_instead_of_false_zeroes() {
        let before = ProcessSnapshot {
            user_cpu_ticks: Some(20),
            system_cpu_ticks: Some(30),
            io: Some(ProcessIoSnapshot {
                read_bytes: 100,
                write_bytes: 200,
                cancelled_write_bytes: 10,
                write_chars: 300,
                write_syscalls: 40,
                ..ProcessIoSnapshot::default()
            }),
            ..ProcessSnapshot::default()
        };
        let after = ProcessSnapshot {
            user_cpu_ticks: Some(19),
            system_cpu_ticks: Some(35),
            io: Some(ProcessIoSnapshot {
                read_bytes: 150,
                write_bytes: 199,
                cancelled_write_bytes: 15,
                write_chars: 350,
                write_syscalls: 39,
                ..ProcessIoSnapshot::default()
            }),
            ..ProcessSnapshot::default()
        };

        let delta = process_delta(&before, &after, Duration::from_secs(1), Some(100));
        assert_eq!(delta.user_cpu_ticks, None);
        assert_eq!(delta.system_cpu_ticks, Some(5));
        assert_eq!(delta.cpu_seconds, None);
        assert_eq!(delta.read_bytes, Some(50));
        assert_eq!(delta.write_bytes, None);
        assert_eq!(delta.cancelled_write_bytes, Some(5));
        assert_eq!(delta.write_chars, Some(50));
        assert_eq!(delta.write_syscalls, None);
    }

    #[test]
    fn rss_sampler_shutdown_interrupts_the_sampling_interval() {
        let sampler = RssSampler::start(Duration::from_secs(2));
        let started = Instant::now();
        let summary = sampler.finish();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(summary.samples >= 1);
    }

    #[test]
    fn file_tree_does_not_follow_symlinks_or_double_count_hardlinks() {
        let temp = tempdir().expect("temporary directory");
        let data = temp.path().join("data");
        fs::write(&data, vec![1u8; 100]).expect("write fixture");
        #[cfg(unix)]
        {
            std::fs::hard_link(&data, temp.path().join("hard")).expect("hard link");
            std::os::unix::fs::symlink(&data, temp.path().join("sym")).expect("symlink");
        }

        let snapshot = capture_file_tree(temp.path()).expect("capture tree");
        #[cfg(unix)]
        {
            assert_eq!(snapshot.regular_files, 1);
            assert_eq!(snapshot.symlinks, 1);
        }
        assert_eq!(snapshot.logical_bytes, 100);
    }
}
