use std::path::Path;

use anyhow::{Context, Result, ensure};
use neo_state_packs::{
    PackCommitHorizon, PackMaterializedViewEvidence, PackStore, PackStoreConfig,
};

const MIN_AUTHORITY_MUTATION_LOOKUP_SAMPLES: usize = 100_000;

pub(super) fn print_materialized_evidence(label: &str, evidence: &PackMaterializedViewEvidence) {
    println!(
        "materialized evidence ({label}): generation={} live_runs={} source_records={} tip_epoch={} tip_segment={} tip_frame_end={} winners={} puts={} tombstones={} value_bytes={} winner_records_sha256=0x{}",
        evidence.generation,
        evidence.live_runs,
        evidence.source_records,
        evidence.tip_epoch,
        evidence.tip_segment_id,
        evidence.tip_frame_end,
        evidence.winner_records,
        evidence.puts,
        evidence.tombstones,
        evidence.value_bytes,
        hex::encode(evidence.winner_records_sha256),
    );
    println!(
        "lookup evidence ({label}): requested={} sampled={} sample_keys_sha256=0x{} present={} absent={} value_bytes={} batches={} point_checks={} synthetic_misses={} sha256=0x{}",
        evidence.lookup_sample_requested,
        evidence.lookup_sampled_keys,
        hex::encode(evidence.sample_keys_sha256),
        evidence.lookup_present,
        evidence.lookup_absent,
        evidence.lookup_value_bytes,
        evidence.lookup_batches,
        evidence.point_checks,
        evidence.synthetic_miss_checks,
        hex::encode(evidence.lookup_sha256),
    );
    println!(
        "frame reference ({label}): sampled={} scrub_frames={} scrub_rows={} scrub_payload_bytes={} sha256=0x{}",
        evidence.frame_reference_keys,
        evidence.frame_scrub.frames,
        evidence.frame_scrub.rows,
        evidence.frame_scrub.payload_bytes,
        hex::encode(evidence.frame_reference_sha256),
    );
    println!(
        "evidence timing ({label}): winner_merge_ns={} frame_reference_ns={} lookup_ns={} total_ns={}",
        evidence.winner_merge_wall_ns,
        evidence.frame_reference_wall_ns,
        evidence.lookup_wall_ns,
        evidence.total_wall_ns,
    );
}

pub(super) fn scrub_indexes_with_label(label: &str, pack: &PackStore) -> Result<()> {
    let stats = pack
        .scrub_index_runs()
        .context("scrub every live authoritative index run")?;
    println!(
        "scrubbed indexes ({label}): runs={} v5_runs={} records={} record_bytes={}",
        stats.runs, stats.v5_runs, stats.records, stats.record_bytes,
    );
    Ok(())
}

pub(super) fn validate_authority_mutation_flags(
    maintain: bool,
    gc: bool,
    scrub_indexes: bool,
    lookup_digest_samples: usize,
) -> Result<()> {
    ensure!(!(maintain && gc), "--maintain conflicts with --gc");
    if maintain || gc {
        ensure!(
            lookup_digest_samples >= MIN_AUTHORITY_MUTATION_LOOKUP_SAMPLES,
            "authority mutation requires --lookup-digest-samples of at least {MIN_AUTHORITY_MUTATION_LOOKUP_SAMPLES} for pre/post/reopen proof"
        );
        ensure!(
            scrub_indexes,
            "authority mutation requires --scrub-indexes before and after publication"
        );
    }
    Ok(())
}

pub(super) fn gc_and_reopen_with_evidence(
    mut pack: PackStore,
    pack_path: &Path,
    config: PackStoreConfig,
    horizon: PackCommitHorizon,
    lookup_digest_samples: usize,
    before: &PackMaterializedViewEvidence,
) -> Result<PackStore> {
    let stats = pack.gc().context("reclaim superseded pack indexes")?;
    println!(
        "GC: runs_deleted={} manifests_deleted={} bytes_reclaimed={}",
        stats.runs_deleted, stats.manifests_deleted, stats.bytes_reclaimed,
    );
    drop(pack);
    let reopened = PackStore::open_at_commit_horizon(pack_path, config, Some(horizon))
        .context("reopen authoritative packs after GC")?;
    let after = reopened
        .materialized_view_evidence(lookup_digest_samples)
        .context("produce post-GC materialized-view evidence")?;
    print_materialized_evidence("post-gc-reopen", &after);
    ensure!(
        before.state_matches(&after),
        "materialized winner or lookup evidence changed across GC"
    );
    scrub_indexes_with_label("post-gc-reopen", &reopened)?;
    println!("GC equivalence: pre/post-reopen evidence matches exactly");
    Ok(reopened)
}
