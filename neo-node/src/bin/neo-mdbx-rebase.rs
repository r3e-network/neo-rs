//! Offline verified MDBX rebase for authoritative StateService node packs.

use anyhow::{Context, Result, ensure};
use clap::Parser;
use neo_node::NodeLifecycleLock;
use neo_state_service::{MDBX_STATE_SERVICE_NAMESPACE, MPT_NODE_KEY_BYTES, MPT_NODE_PREFIX};
use neo_storage::mdbx::{
    MdbxExactKeyExclusion, MdbxRebaseOptions, finalize_mdbx_rebase, rebase_mdbx_environment,
};
use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

const MAINTENANCE_TABLE: &str = "neo_node_metadata";

#[derive(Debug, Parser)]
#[command(
    name = "neo-mdbx-rebase",
    about = "Copy Neo MDBX while removing pack-authoritative legacy MPT rows"
)]
struct Cli {
    /// Offline source MDBX directory.
    #[arg(long)]
    source: PathBuf,

    /// Fresh destination MDBX directory. It must not exist.
    #[arg(long)]
    destination: PathBuf,

    /// Fresh JSON evidence report path.
    #[arg(long)]
    report: PathBuf,

    /// Maximum source rows scanned in one frozen transaction.
    #[arg(long, default_value_t = 1_000_000)]
    batch_rows: u64,

    /// Maximum retained MiB buffered for one durable destination commit.
    #[arg(long, default_value_t = 256)]
    batch_mib: usize,

    /// Destination MDBX upper geometry in GiB.
    #[arg(long, default_value_t = 512)]
    geometry_upper_gib: isize,

    /// Destination MDBX growth step in MiB.
    #[arg(long, default_value_t = 256)]
    geometry_growth_mib: isize,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_target(false)
        .with_writer(std::io::stderr)
        .init();
    let cli = Cli::parse();
    validate_report_path(&cli)?;
    let _source_lifecycle_lock = NodeLifecycleLock::acquire(&cli.source)
        .context("acquire exclusive ownership of the source MDBX")?;
    ensure!(
        !cli.report.exists(),
        "report path {} already exists",
        cli.report.display()
    );
    let mut options = MdbxRebaseOptions::new(
        &cli.source,
        &cli.destination,
        vec![
            MAINTENANCE_TABLE.to_owned(),
            MDBX_STATE_SERVICE_NAMESPACE.to_owned(),
        ],
        MdbxExactKeyExclusion::new(
            MDBX_STATE_SERVICE_NAMESPACE,
            vec![MPT_NODE_PREFIX],
            MPT_NODE_KEY_BYTES,
        ),
    );
    options.batch_scanned_rows = cli.batch_rows;
    options.batch_retained_bytes = cli
        .batch_mib
        .checked_mul(1024 * 1024)
        .context("--batch-mib overflows usize")?;
    options.geometry_upper_bytes = cli
        .geometry_upper_gib
        .checked_mul(1024 * 1024 * 1024)
        .context("--geometry-upper-gib overflows isize")?;
    options.geometry_growth_bytes = cli
        .geometry_growth_mib
        .checked_mul(1024 * 1024)
        .context("--geometry-growth-mib overflows isize")?;

    let report = rebase_mdbx_environment(&options).context("rebase MDBX environment")?;
    write_report(&cli.report, &serde_json::to_vec_pretty(&report)?)?;
    finalize_mdbx_rebase(&cli.destination).context("publish verified MDBX rebase")?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

fn write_report(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    ensure!(
        parent.is_dir(),
        "report parent {} is absent",
        parent.display()
    );
    let file_name = path
        .file_name()
        .context("report path must have a file name")?
        .to_string_lossy();
    let temporary = parent.join(format!(".{file_name}.tmp"));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary)
        .with_context(|| format!("create temporary report {}", temporary.display()))?;
    file.write_all(bytes)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    if let Err(error) = fs::hard_link(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error).with_context(|| {
            format!(
                "publish report {} without replacing an existing path",
                path.display()
            )
        });
    }
    fs::remove_file(&temporary)?;
    File::open(parent)?.sync_all()?;
    Ok(())
}

fn validate_report_path(cli: &Cli) -> Result<()> {
    let source = fs::canonicalize(&cli.source)
        .with_context(|| format!("canonicalize source {}", cli.source.display()))?;
    let destination = normalize_new_path(&cli.destination)?;
    let report = normalize_new_path(&cli.report)?;
    ensure!(
        !report.starts_with(&source) && !report.starts_with(&destination),
        "report {} must be outside source {} and destination {}",
        report.display(),
        source.display(),
        destination.display()
    );
    Ok(())
}

fn normalize_new_path(path: &Path) -> Result<PathBuf> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let parent = fs::canonicalize(parent)
        .with_context(|| format!("canonicalize parent {}", parent.display()))?;
    let file_name = path
        .file_name()
        .with_context(|| format!("path {} has no final component", path.display()))?;
    Ok(parent.join(file_name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn cli(source: PathBuf, destination: PathBuf, report: PathBuf) -> Cli {
        Cli {
            source,
            destination,
            report,
            batch_rows: 1,
            batch_mib: 1,
            geometry_upper_gib: 1,
            geometry_growth_mib: 1,
        }
    }

    #[test]
    fn report_path_must_be_outside_both_database_directories() {
        let temporary = TempDir::new().expect("tempdir");
        let source = temporary.path().join("source");
        fs::create_dir(&source).expect("source");
        let destination = temporary.path().join("destination");
        let outside = temporary.path().join("report.json");
        assert!(validate_report_path(&cli(source.clone(), destination.clone(), outside)).is_ok());
        let source_report = source.join("report.json");
        assert!(
            validate_report_path(&cli(source.clone(), destination.clone(), source_report)).is_err()
        );
        let destination_report = destination.join("mdbx.dat");
        assert!(validate_report_path(&cli(source, destination, destination_report)).is_err());
    }

    #[test]
    fn report_publication_never_replaces_a_racing_target() {
        let temporary = TempDir::new().expect("tempdir");
        let report = temporary.path().join("report.json");
        fs::write(&report, b"existing").expect("seed report");
        let error = write_report(&report, b"replacement").expect_err("must not replace");
        assert!(error.to_string().contains("without replacing"), "{error}");
        assert_eq!(fs::read(&report).expect("read report"), b"existing");
    }
}
