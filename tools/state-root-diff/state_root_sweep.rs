//! Strictly read-only state-root sweep for an already completed replay.
//!
//! Task 59: Task 52 needs to verify a finished MainNet replay from outside the
//! node, without any possibility of touching the database that produced it.
//!
//! # Safety contract
//!
//! * The database is opened with `DB::open_for_read_only` (default) or
//!   `DB::open_as_secondary`; the read-write `RocksDbStore::open` path is never
//!   used and is not reachable from this binary.
//! * No compaction, flush, repair, WAL replay/write, checkpoint, migration or
//!   schema change is requested. There is no `--write` flag and no mutation
//!   option at all, so an operator cannot opt out of read-only behaviour.
//! * All reads go through one bounded forward iterator over the state-root key
//!   range plus point lookups of the two progress cursors.
//! * The reference file is streamed into a disk-backed height index, so memory
//!   stays bounded even though the reference holds 13.15M rows in unordered
//!   order (see `sweep/reference_index.rs`).
//!
//! # Exit codes
//!
//! * `0` — every requested height was compared and matched.
//! * `1` — verification failure: mismatch, missing local/reference height,
//!   undecodable record, corrupt reference, or an incomplete sweep.
//! * `2` — usage error: clap rejected the arguments, or the arguments were
//!   refused by this tool (empty paths, secondary mode without an external
//!   scratch path, a mutation-like combination).
//! * `3` — operational error: the database or reference could not be opened or
//!   read, or the result file could not be written.

#[path = "sweep/reference.rs"]
mod reference;
#[path = "sweep/reference_index.rs"]
mod reference_index;
#[path = "sweep/sweep.rs"]
mod sweep_core;
#[cfg(test)]
#[path = "sweep/tests.rs"]
mod tests;

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result, bail};
use clap::{Parser, ValueEnum};
use rocksdb::{DB, Options};
use serde::Serialize;

use neo_state_service::record_schema;

use crate::reference_index::HeightHashIndex;
use crate::sweep_core::{Mismatch, Observation, SweepConfig};

const TOOL: &str = "state-root-sweep";
/// Every requested height matched.
const EXIT_OK: i32 = 0;
/// Mismatch, missing height, decode failure, corrupt reference or partial sweep.
const EXIT_VERIFICATION_FAILED: i32 = 1;
/// Arguments were refused before any database was opened.
const EXIT_USAGE: i32 = 2;
/// The database/reference/output could not be read or written.
const EXIT_OPERATIONAL_ERROR: i32 = 3;

/// How the database handle is obtained.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum OpenMode {
    /// `DB::open_for_read_only` — the primary directory is opened without any
    /// write permission path in RocksDB.
    ReadOnly,
    /// `DB::open_as_secondary` — RocksDB tails the primary through a
    /// caller-supplied scratch directory; nothing is written to the primary.
    Secondary,
}

impl OpenMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read-only",
            Self::Secondary => "secondary",
        }
    }

    fn open_api(self) -> &'static str {
        match self {
            Self::ReadOnly => "DB::open_for_read_only(error_if_log_file_exist=false)",
            Self::Secondary => "DB::open_as_secondary + try_catch_up_with_primary",
        }
    }
}

#[derive(Debug, Clone, Parser)]
#[command(
    name = "state-root-sweep",
    about = "Compare persisted state roots against a reference JSONL without mutating the database",
    long_about = None,
    after_long_help = "The tool is strictly read-only: the database is opened with \
DB::open_for_read_only (or DB::open_as_secondary) and no compaction, flush, repair, WAL replay, \
checkpoint, migration or schema change is ever requested. There is no --write or any other \
mutation option.\n\nExit codes: 0 = every requested height compared and matched, \
1 = mismatch/missing height/decode failure/corrupt reference/incomplete sweep, 2 = usage error, \
3 = operational error (database, reference or output unreadable/unwritable)."
)]
struct Args {
    /// RocksDB directory holding the chain and state service records.
    #[arg(long)]
    db: PathBuf,
    /// Reference JSONL with one {"height", "roothash"} object per line.
    #[arg(long)]
    reference: PathBuf,
    /// First height compared (inclusive).
    #[arg(long, default_value_t = 0)]
    start_height: u32,
    /// Last height compared (inclusive). Defaults to the local state-root
    /// cursor, then to the highest height present in the reference.
    #[arg(long)]
    end_height: Option<u32>,
    /// Path of the machine-readable JSON result.
    #[arg(long)]
    output: PathBuf,
    /// Database open mode.
    #[arg(long, value_enum, default_value_t = OpenMode::ReadOnly)]
    mode: OpenMode,
    /// Scratch directory for --mode secondary (required there, refused otherwise).
    #[arg(long)]
    secondary_path: Option<PathBuf>,
    /// Stop at the first hash mismatch instead of collecting every mismatch.
    #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
    stop_on_first_mismatch: bool,
    /// Report progress every N heights (also used for reference rows).
    #[arg(long, default_value_t = 100_000)]
    progress_every: u64,
    /// Directory for the temporary height index (defaults to the output parent).
    #[arg(long)]
    temp_dir: Option<PathBuf>,
    /// Keep the temporary height index instead of deleting it.
    #[arg(long)]
    keep_temp: bool,
}

impl Args {
    fn validate(&self) -> Result<()> {
        if self.db.as_os_str().is_empty() {
            bail!("--db must not be empty");
        }
        if !self.db.is_dir() {
            bail!(
                "--db {} is not a readable database directory",
                self.db.display()
            );
        }
        if self.reference.as_os_str().is_empty() || !self.reference.is_file() {
            bail!(
                "--reference {} is not a readable file",
                self.reference.display()
            );
        }
        if self.output.as_os_str().is_empty() {
            bail!("--output must not be empty");
        }
        if self.progress_every == 0 {
            bail!("--progress-every must be greater than zero");
        }
        match (self.mode, &self.secondary_path) {
            (OpenMode::Secondary, None) => {
                bail!("--mode secondary requires --secondary-path (a scratch directory)")
            }
            (OpenMode::ReadOnly, Some(path)) => {
                bail!(
                    "--secondary-path {} is only valid with --mode secondary",
                    path.display()
                )
            }
            (OpenMode::Secondary, Some(secondary)) => {
                let db = std::path::absolute(&self.db).unwrap_or_else(|_| self.db.clone());
                let secondary =
                    std::path::absolute(secondary).unwrap_or_else(|_| secondary.clone());
                if secondary == db || secondary.starts_with(&db) {
                    bail!(
                        "--secondary-path {} must be outside --db {}: RocksDB writes its \
                         secondary copies there and the primary must stay untouched",
                        secondary.display(),
                        db.display()
                    );
                }
            }
            (OpenMode::ReadOnly, None) => {}
        }
        Ok(())
    }
}

/// An opened, read-only database handle plus the facts reported in the result.
struct Database {
    db: DB,
    open_mode: &'static str,
    open_api: &'static str,
}

fn open_database(args: &Args) -> Result<Database> {
    let mut options = Options::default();
    // Never create, never overwrite, never repair: a missing database or a
    // damaged manifest must fail instead of being "fixed" behind the operator's
    // back.
    options.create_if_missing(false);
    options.set_error_if_exists(false);
    options.set_paranoid_checks(true);
    // Belt and braces: even if a caller later reuses these options for a
    // writable handle, background compaction and WAL writes stay disabled.
    // Read-only handles cannot write at all.
    options.set_disable_auto_compactions(true);
    options.set_max_open_files(1_024);

    match args.mode {
        OpenMode::ReadOnly => {
            // `error_if_log_file_exist=false`: a database stopped after an
            // unclean shutdown still carries WAL `*.log` files. Read-only open
            // never performs WAL recovery, so tolerating them cannot replay or
            // write anything, while refusing them would block exactly the
            // post-crash databases QA must inspect.
            let db = DB::open_for_read_only(&options, &args.db, false)
                .with_context(|| format!("failed to open {} read-only", args.db.display()))?;
            Ok(Database {
                db,
                open_mode: OpenMode::ReadOnly.as_str(),
                open_api: OpenMode::ReadOnly.open_api(),
            })
        }
        OpenMode::Secondary => {
            let secondary = args
                .secondary_path
                .as_ref()
                .context("--mode secondary requires --secondary-path")?;
            std::fs::create_dir_all(secondary).with_context(|| {
                format!("failed to create secondary path {}", secondary.display())
            })?;
            let db = DB::open_as_secondary(&options, &args.db, secondary).with_context(|| {
                format!(
                    "failed to open {} as a secondary through {}",
                    args.db.display(),
                    secondary.display()
                )
            })?;
            db.try_catch_up_with_primary()
                .context("secondary failed to catch up with the primary")?;
            Ok(Database {
                db,
                open_mode: OpenMode::Secondary.as_str(),
                open_api: OpenMode::Secondary.open_api(),
            })
        }
    }
}

/// Windowed + cumulative rate reporter.
struct Reporter {
    label: &'static str,
    every: u64,
    next: u64,
    last_count: u64,
    last_at: Instant,
    started: Instant,
}

impl Reporter {
    fn new(label: &'static str, every: u64) -> Self {
        let now = Instant::now();
        Self {
            label,
            every: every.max(1),
            next: every.max(1),
            last_count: 0,
            last_at: now,
            started: now,
        }
    }

    fn observe(&mut self, count: u64, height: Option<u32>) {
        if count < self.next {
            return;
        }
        let now = Instant::now();
        let window = now.duration_since(self.last_at).as_secs_f64().max(1e-9);
        let cumulative = now.duration_since(self.started).as_secs_f64().max(1e-9);
        let window_items = count - self.last_count;
        let height = height.map_or_else(|| "n/a".to_owned(), |h| h.to_string());
        println!(
            "[progress] {} count={count} height={height} window={window_items} in {window:.2}s \
             ({:.0}/s) cumulative={count} in {cumulative:.2}s ({:.0}/s)",
            self.label,
            window_items as f64 / window,
            count as f64 / cumulative,
        );
        self.last_count = count;
        self.last_at = now;
        self.next = count.saturating_add(self.every);
    }
}

#[derive(Debug, Serialize)]
struct Report {
    tool: &'static str,
    version: &'static str,
    verdict: &'static str,
    exit_code: i32,
    requested: RequestedRange,
    db: DbReport,
    reference: ReferenceReport,
    comparison: ComparisonReport,
    local_cursor: CursorReport,
    timing: TimingReport,
    temp_index: TempIndexReport,
}

#[derive(Debug, Serialize)]
struct RequestedRange {
    start_height: u32,
    end_height: u32,
    end_height_source: &'static str,
    total_heights: u64,
}

#[derive(Debug, Serialize)]
struct DbReport {
    path: String,
    open_mode: &'static str,
    open_api: &'static str,
    secondary_path: Option<String>,
    key_schema: &'static str,
}

#[derive(Debug, Serialize)]
struct ReferenceReport {
    path: String,
    provenance: String,
    rows_read: u64,
    rows_indexed: u64,
    rows_below_start: u64,
    rows_above_end: u64,
    observed_ascending: bool,
    duplicate_identical_heights: u64,
    duplicate_conflicting_heights: u64,
    first_conflict_height: Option<u32>,
}

#[derive(Debug, Serialize)]
struct ComparisonReport {
    compared_heights: u64,
    matches: u64,
    mismatches: u64,
    missing_local: u64,
    missing_reference: u64,
    decode_errors: u64,
    first_mismatch: Option<MismatchReport>,
    first_missing_local_height: Option<u32>,
    first_missing_reference_height: Option<u32>,
    first_local_height: Option<u32>,
    last_local_height: Option<u32>,
    sweep_complete: bool,
}

#[derive(Debug, Serialize)]
struct MismatchReport {
    height: u32,
    local: String,
    reference: String,
}

#[derive(Debug, Serialize)]
struct CursorReport {
    key_hex: String,
    local_root_index: Option<u32>,
    validated_root_index: Option<u32>,
}

#[derive(Debug, Serialize)]
struct TimingReport {
    elapsed_seconds: f64,
    reference_scan_seconds: f64,
    comparison_seconds: f64,
    heights_per_second: f64,
}

#[derive(Debug, Serialize)]
struct TempIndexReport {
    path: String,
    bytes: u64,
    removed: bool,
}

fn provenance(path: &Path) -> Result<String> {
    let metadata = std::fs::metadata(path)
        .with_context(|| format!("failed to stat reference {}", path.display()))?;
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(|| "unknown".to_owned(), |delta| delta.as_secs().to_string());
    Ok(format!(
        "file={} bytes={} modified_unix={modified}",
        canonical.display(),
        metadata.len()
    ))
}

fn mismatch_report(mismatch: &Mismatch) -> MismatchReport {
    MismatchReport {
        height: mismatch.height,
        local: mismatch.local_hex(),
        reference: mismatch.reference_hex(),
    }
}

fn run(args: &Args) -> Result<Report> {
    args.validate()?;
    let started = Instant::now();

    // Startup line: names the open mode, the exact API, the namespace schema and
    // the fact that no mutation path exists before anything is read.
    println!(
        "[startup] mode={} api=\"{}\" db=\"{}\" reference=\"{}\" schema=\"{}\" \
         mutation_options=none",
        args.mode.as_str(),
        args.mode.open_api(),
        args.db.display(),
        args.reference.display(),
        record_schema::KEY_LAYOUT,
    );
    if args.mode == OpenMode::Secondary {
        println!(
            "[startup] secondary scratch path=\"{}\"",
            args.secondary_path
                .as_ref()
                .map_or_else(String::new, |path| path.display().to_string())
        );
    }

    let provenance = provenance(&args.reference)?;
    let database = open_database(args)?;
    let (local_cursor, validated_cursor) = sweep_core::read_cursors(&database.db)
        .context("failed to read the local state-root cursors")?;

    // Resolve the end height before the reference pass when possible. Without
    // an explicit flag and without a cursor, the reference pass runs open-ended
    // and the highest indexed height becomes the end.
    let (planned_end, planned_source) = match args.end_height {
        Some(end) => (Some(end), "cli"),
        None => (local_cursor, "local_cursor"),
    };
    if args.start_height > planned_end.unwrap_or(u32::MAX) {
        bail!(
            "--start-height {} is above the resolved end height {}",
            args.start_height,
            planned_end.unwrap_or(u32::MAX)
        );
    }

    let temp_dir = args.temp_dir.clone().unwrap_or_else(|| {
        args.output
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .map_or_else(|| PathBuf::from("."), Path::to_path_buf)
    });
    let mut index = HeightHashIndex::create(&temp_dir, args.keep_temp).with_context(|| {
        format!(
            "failed to create the temporary height index in {}",
            temp_dir.display()
        )
    })?;

    let scan_started = Instant::now();
    let mut reference_progress = Reporter::new("reference", args.progress_every);
    let scan = reference::build(
        &args.reference,
        args.start_height,
        planned_end,
        &mut index,
        |rows| reference_progress.observe(rows, None),
    )?;
    index
        .seal()
        .context("failed to finalize the temporary height index")?;
    let reference_scan_seconds = scan_started.elapsed().as_secs_f64();

    let (end, end_source) = match planned_end {
        Some(end) => (end, planned_source),
        None => match scan.max_indexed {
            Some(max) => (max, "reference_max"),
            None => bail!(
                "no --end-height was given and neither the local cursor nor the reference \
                 provides a height at or above --start-height {}",
                args.start_height
            ),
        },
    };

    println!(
        "[reference] rows={} indexed={} below_start={} above_end={} ascending={} max_indexed={:?}",
        scan.rows_read,
        scan.rows_indexed,
        scan.rows_below_start,
        scan.rows_above_end,
        scan.observed_ascending,
        scan.max_indexed,
    );

    let config = SweepConfig {
        start: args.start_height,
        end,
        stop_on_first_mismatch: args.stop_on_first_mismatch,
    };
    let mut sweep_progress = Reporter::new("sweep", args.progress_every);
    let outcome = sweep_core::sweep(
        &database.db,
        &mut index,
        &config,
        |observation: Observation| {
            sweep_progress.observe(observation.processed, Some(observation.height));
        },
    )?;

    let index_stats = index.stats().clone();
    let index_bytes = index.allocated_bytes();
    let index_path = index.path().display().to_string();
    drop(index);

    let total_heights = config.total_heights();
    let clean = outcome.is_clean()
        && index_stats.duplicate_conflicting == 0
        && scan.rows_read > 0
        // A sweep that compared nothing cannot be called a pass.
        && outcome.compared == total_heights;
    let exit_code = if clean {
        EXIT_OK
    } else {
        EXIT_VERIFICATION_FAILED
    };
    let elapsed = started.elapsed().as_secs_f64();

    let report = Report {
        tool: TOOL,
        version: env!("CARGO_PKG_VERSION"),
        verdict: if clean { "pass" } else { "fail" },
        exit_code,
        requested: RequestedRange {
            start_height: config.start,
            end_height: end,
            end_height_source: end_source,
            total_heights,
        },
        db: DbReport {
            path: args.db.display().to_string(),
            open_mode: database.open_mode,
            open_api: database.open_api,
            secondary_path: args
                .secondary_path
                .as_ref()
                .map(|path| path.display().to_string()),
            key_schema: record_schema::KEY_LAYOUT,
        },
        reference: ReferenceReport {
            path: args.reference.display().to_string(),
            provenance,
            rows_read: scan.rows_read,
            rows_indexed: scan.rows_indexed,
            rows_below_start: scan.rows_below_start,
            rows_above_end: scan.rows_above_end,
            observed_ascending: scan.observed_ascending,
            duplicate_identical_heights: index_stats.duplicate_identical,
            duplicate_conflicting_heights: index_stats.duplicate_conflicting,
            first_conflict_height: index_stats.first_conflict_height,
        },
        comparison: ComparisonReport {
            compared_heights: outcome.compared,
            matches: outcome.matches,
            mismatches: outcome.mismatches,
            missing_local: outcome.missing_local,
            missing_reference: outcome.missing_reference,
            decode_errors: outcome.decode_errors,
            first_mismatch: outcome.first_mismatch.as_ref().map(mismatch_report),
            first_missing_local_height: outcome.first_missing_local,
            first_missing_reference_height: outcome.first_missing_reference,
            first_local_height: outcome.first_local_height,
            last_local_height: outcome.last_local_height,
            sweep_complete: outcome.sweep_complete,
        },
        local_cursor: CursorReport {
            key_hex: to_hex(&record_schema::local_root_cursor_key().to_array()),
            local_root_index: local_cursor,
            validated_root_index: validated_cursor,
        },
        timing: TimingReport {
            elapsed_seconds: elapsed,
            reference_scan_seconds,
            comparison_seconds: outcome.elapsed.as_secs_f64(),
            heights_per_second: if outcome.elapsed.as_secs_f64() > 0.0 {
                outcome.compared as f64 / outcome.elapsed.as_secs_f64()
            } else {
                0.0
            },
        },
        temp_index: TempIndexReport {
            path: index_path,
            bytes: index_bytes,
            removed: !args.keep_temp,
        },
    };

    write_report(&args.output, &report)?;

    println!(
        "[result] verdict={} compared={} matches={} mismatches={} missing_local={} \
         missing_reference={} decode_errors={} sweep_complete={}",
        report.verdict,
        report.comparison.compared_heights,
        report.comparison.matches,
        report.comparison.mismatches,
        report.comparison.missing_local,
        report.comparison.missing_reference,
        report.comparison.decode_errors,
        report.comparison.sweep_complete,
    );
    if let Some(mismatch) = &report.comparison.first_mismatch {
        println!(
            "[result] first_mismatch height={} local={} reference={}",
            mismatch.height, mismatch.local, mismatch.reference
        );
    }
    if let Some(height) = report.comparison.first_missing_local_height {
        println!("[result] first_missing_local height={height}");
    }
    if let Some(height) = report.comparison.first_missing_reference_height {
        println!("[result] first_missing_reference height={height}");
    }
    println!(
        "[result] cursor_key={} local_root_index={:?} validated_root_index={:?}",
        report.local_cursor.key_hex,
        report.local_cursor.local_root_index,
        report.local_cursor.validated_root_index
    );
    println!(
        "[result] elapsed={:.2}s reference_scan={:.2}s comparison={:.2}s temp_index_bytes={}",
        elapsed, reference_scan_seconds, report.timing.comparison_seconds, index_bytes
    );
    println!("[result] wrote {}", args.output.display());

    Ok(report)
}

fn write_report(path: &Path, report: &Report) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let payload =
        serde_json::to_string_pretty(report).context("failed to serialize the sweep result")?;
    std::fs::write(path, format!("{payload}\n"))
        .with_context(|| format!("failed to write {}", path.display()))
}

fn write_error_report(args: &Args, exit_code: i32, message: &str) {
    if args.output.as_os_str().is_empty() {
        return;
    }
    let payload = serde_json::json!({
        "tool": TOOL,
        "version": env!("CARGO_PKG_VERSION"),
        "verdict": "error",
        "exit_code": exit_code,
        "db": {
            "path": args.db.display().to_string(),
            "open_mode": args.mode.as_str(),
            "open_api": args.mode.open_api(),
        },
        "reference": {
            "path": args.reference.display().to_string(),
        },
        "error": message,
    });
    if let Some(parent) = args.output.parent()
        && !parent.as_os_str().is_empty()
    {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(text) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(&args.output, format!("{text}\n"));
    }
}

fn to_hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

fn run_cli(args: Args) -> i32 {
    // Refused arguments never reach a database handle, so they are reported as
    // usage errors rather than operational ones.
    if let Err(error) = args.validate() {
        eprintln!("{TOOL}: usage error: {error:#}");
        write_error_report(&args, EXIT_USAGE, &format!("usage error: {error:#}"));
        return EXIT_USAGE;
    }
    match run(&args) {
        Ok(report) => report.exit_code,
        Err(error) => {
            eprintln!("{TOOL}: error: {error:#}");
            write_error_report(&args, EXIT_OPERATIONAL_ERROR, &format!("{error:#}"));
            EXIT_OPERATIONAL_ERROR
        }
    }
}

fn main() {
    let args = Args::parse();
    std::process::exit(run_cli(args));
}
