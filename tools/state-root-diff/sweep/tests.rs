//! Tests for the read-only state-root sweep.
//!
//! Fixtures are *test-owned* RocksDB databases created here with
//! `rocksdb::DB::open`; the production binary itself never links a writable
//! open path. Records are written with the canonical schema from
//! `neo_state_service::record_schema`, so a schema drift breaks these tests
//! instead of silently producing a sweep against the wrong bytes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use clap::Parser;
use neo_primitives::UInt256;
use neo_state_service::StateRoot;
use neo_state_service::record_schema;

use crate::{Args, EXIT_OK, EXIT_USAGE, EXIT_VERIFICATION_FAILED, OpenMode, run, run_cli};

fn hash_for(height: u32, salt: u8) -> UInt256 {
    let mut bytes = [0u8; 32];
    bytes[..4].copy_from_slice(&height.to_be_bytes());
    bytes[4] = salt;
    bytes[31] = 0x5a;
    UInt256::from(bytes)
}

fn write_database(dir: &Path, roots: &[(u32, UInt256)], cursor: Option<u32>) {
    let mut options = rocksdb::Options::default();
    options.create_if_missing(true);
    let db = rocksdb::DB::open(&options, dir).expect("create fixture database");
    for (height, hash) in roots {
        let root = StateRoot::new_current(*height, *hash);
        db.put(
            record_schema::state_root_record_key(*height).to_array(),
            record_schema::encode_state_root_record(&root),
        )
        .expect("write fixture root record");
    }
    if let Some(index) = cursor {
        db.put(
            record_schema::local_root_cursor_key().to_array(),
            record_schema::encode_cursor(index),
        )
        .expect("write fixture local cursor");
    }
    db.flush().expect("flush fixture database");
}

fn write_raw_value(dir: &Path, key: Vec<u8>, value: Vec<u8>) {
    let options = rocksdb::Options::default();
    let db = rocksdb::DB::open(&options, dir).expect("reopen fixture database");
    db.put(key, value).expect("write fixture value");
    db.flush().expect("flush fixture database");
}

fn write_reference(path: &Path, rows: &[(u32, UInt256)]) {
    let mut text = String::new();
    for (height, hash) in rows {
        text.push_str(&format!(
            "{{\"height\": {height}, \"roothash\": \"{hash}\"}}\n"
        ));
    }
    std::fs::write(path, text).expect("write reference file");
}

fn write_reference_lines(path: &Path, lines: &[String]) {
    std::fs::write(path, lines.join("\n") + "\n").expect("write reference file");
}

fn args(db: &Path, reference: &Path, output: &Path) -> Args {
    Args {
        db: db.to_path_buf(),
        reference: reference.to_path_buf(),
        start_height: 0,
        end_height: None,
        output: output.to_path_buf(),
        mode: OpenMode::ReadOnly,
        secondary_path: None,
        stop_on_first_mismatch: true,
        progress_every: 4,
        temp_dir: None,
        keep_temp: false,
    }
}

fn report(output: &Path) -> serde_json::Value {
    let text = std::fs::read_to_string(output).expect("result file exists");
    serde_json::from_str(&text).expect("result file is JSON")
}

/// Data files of a database directory with their exact bytes.
///
/// `LOG*` is skipped: it is RocksDB's own text status log, which a read-only
/// open may append a status line to. Every data-bearing byte — SST, MANIFEST,
/// WAL (`*.log`), `CURRENT`, `IDENTITY`, `OPTIONS-*` — must match exactly.
fn data_files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in std::fs::read_dir(dir).expect("read database directory") {
        let path = entry.expect("directory entry").path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_default();
        if name.starts_with("LOG") {
            continue;
        }
        files.insert(name, std::fs::read(&path).expect("read database file"));
    }
    files
}

fn sequence_number(dir: &Path) -> u64 {
    let options = rocksdb::Options::default();
    let db = rocksdb::DB::open(&options, dir).expect("open fixture database");
    let sequence = db.latest_sequence_number();
    drop(db);
    sequence
}

fn fixture() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("state-root-sweep-")
        .tempdir()
        .expect("temporary directory")
}

fn seeds(from: u32, to: u32, salt: u8) -> Vec<(u32, UInt256)> {
    (from..=to).map(|h| (h, hash_for(h, salt))).collect()
}

#[test]
fn exact_match_range_passes() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    write_database(&db, &seeds(0, 4, 1), Some(4));
    write_reference(&reference, &seeds(0, 4, 1));

    let result = run(&args(&db, &reference, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_OK);
    assert_eq!(result.verdict, "pass");

    let json = report(&output);
    assert_eq!(json["verdict"], "pass");
    assert_eq!(json["exit_code"], EXIT_OK);
    assert_eq!(json["requested"]["start_height"], 0);
    assert_eq!(json["requested"]["end_height"], 4);
    assert_eq!(json["requested"]["end_height_source"], "local_cursor");
    assert_eq!(json["requested"]["total_heights"], 5);
    assert_eq!(json["comparison"]["compared_heights"], 5);
    assert_eq!(json["comparison"]["matches"], 5);
    assert_eq!(json["comparison"]["mismatches"], 0);
    assert_eq!(json["comparison"]["missing_local"], 0);
    assert_eq!(json["comparison"]["missing_reference"], 0);
    assert_eq!(json["comparison"]["decode_errors"], 0);
    assert_eq!(json["comparison"]["sweep_complete"], true);
    assert_eq!(
        json["comparison"]["first_mismatch"],
        serde_json::Value::Null
    );
    assert_eq!(json["db"]["open_mode"], "read-only");
    assert!(
        json["db"]["open_api"]
            .as_str()
            .expect("open api string")
            .contains("open_for_read_only")
    );
    assert_eq!(json["local_cursor"]["local_root_index"], 4);
    assert_eq!(json["local_cursor"]["key_hex"], "00000080f2");
    assert!(
        json["reference"]["provenance"]
            .as_str()
            .expect("provenance")
            .contains("bytes=")
    );
    assert!(
        json["temp_index"]["removed"]
            .as_bool()
            .expect("removed flag")
    );
}

#[test]
fn first_mismatch_stops_and_fails() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    write_database(&db, &seeds(0, 4, 1), Some(4));
    // Height 3 differs: reference claims a different root.
    let mut rows = seeds(0, 4, 1);
    rows[3].1 = hash_for(3, 9);
    write_reference(&reference, &rows);

    let result = run(&args(&db, &reference, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_VERIFICATION_FAILED);
    assert_eq!(result.verdict, "fail");

    let json = report(&output);
    assert_eq!(json["comparison"]["mismatches"], 1);
    assert_eq!(json["comparison"]["compared_heights"], 4);
    assert_eq!(json["comparison"]["sweep_complete"], false);
    assert_eq!(json["comparison"]["first_mismatch"]["height"], 3);
    assert_eq!(
        json["comparison"]["first_mismatch"]["local"],
        hash_for(3, 1).to_string()
    );
    assert_eq!(
        json["comparison"]["first_mismatch"]["reference"],
        hash_for(3, 9).to_string()
    );
}

#[test]
fn collect_all_reports_every_mismatch() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    write_database(&db, &seeds(0, 5, 1), Some(5));
    let mut rows = seeds(0, 5, 1);
    rows[1].1 = hash_for(1, 7);
    rows[4].1 = hash_for(4, 7);
    write_reference(&reference, &rows);

    let mut run_args = args(&db, &reference, &output);
    run_args.stop_on_first_mismatch = false;
    let result = run(&run_args).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_VERIFICATION_FAILED);

    let json = report(&output);
    assert_eq!(json["comparison"]["mismatches"], 2);
    assert_eq!(json["comparison"]["compared_heights"], 6);
    assert_eq!(json["comparison"]["sweep_complete"], true);
    assert_eq!(json["comparison"]["first_mismatch"]["height"], 1);
}

#[test]
fn missing_local_height_fails() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    // Height 2 is absent locally; the cursor still claims 4 so the range is 0..=4.
    let rows: Vec<(u32, UInt256)> = seeds(0, 4, 1)
        .into_iter()
        .filter(|(height, _)| *height != 2)
        .collect();
    write_database(&db, &rows, Some(4));
    write_reference(&reference, &seeds(0, 4, 1));

    let result = run(&args(&db, &reference, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_VERIFICATION_FAILED);

    let json = report(&output);
    assert_eq!(json["comparison"]["missing_local"], 1);
    assert_eq!(json["comparison"]["first_missing_local_height"], 2);
    assert_eq!(json["comparison"]["missing_reference"], 0);
    assert_eq!(json["comparison"]["compared_heights"], 4);
}

#[test]
fn missing_reference_height_fails() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    write_database(&db, &seeds(0, 4, 1), Some(4));
    let rows: Vec<(u32, UInt256)> = seeds(0, 4, 1)
        .into_iter()
        .filter(|(height, _)| *height != 3)
        .collect();
    write_reference(&reference, &rows);

    let result = run(&args(&db, &reference, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_VERIFICATION_FAILED);

    let json = report(&output);
    assert_eq!(json["comparison"]["missing_reference"], 1);
    assert_eq!(json["comparison"]["first_missing_reference_height"], 3);
    assert_eq!(json["comparison"]["missing_local"], 0);
    assert_eq!(json["comparison"]["compared_heights"], 4);
}

#[test]
fn unordered_reference_matches_without_ordering_assumptions() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    write_database(&db, &seeds(0, 6, 1), Some(6));
    // Deliberately shuffled: production references are not height-ordered.
    let mut rows = seeds(0, 6, 1);
    rows.reverse();
    rows.swap(1, 5);
    write_reference(&reference, &rows);

    let result = run(&args(&db, &reference, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_OK);

    let json = report(&output);
    assert_eq!(json["comparison"]["compared_heights"], 7);
    assert_eq!(json["comparison"]["matches"], 7);
    assert_eq!(json["reference"]["observed_ascending"], false);
    // The height index is a fixed-stride slot file: worst case 33 bytes per
    // height, independent of row order (documented production strategy).
    let bytes = json["temp_index"]["bytes"].as_u64().expect("index size");
    assert_eq!(bytes, 7 * 33);
}

#[test]
fn range_bounds_and_explicit_end_height() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    write_database(&db, &seeds(0, 9, 1), Some(9));
    write_reference(&reference, &seeds(0, 9, 1));

    let mut run_args = args(&db, &reference, &output);
    run_args.start_height = 4;
    run_args.end_height = Some(7);
    let result = run(&run_args).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_OK);

    let json = report(&output);
    assert_eq!(json["requested"]["start_height"], 4);
    assert_eq!(json["requested"]["end_height"], 7);
    assert_eq!(json["requested"]["end_height_source"], "cli");
    assert_eq!(json["requested"]["total_heights"], 4);
    assert_eq!(json["comparison"]["compared_heights"], 4);
    assert_eq!(json["reference"]["rows_below_start"], 4);
    assert_eq!(json["reference"]["rows_above_end"], 2);
    assert_eq!(json["comparison"]["first_local_height"], 4);
    assert_eq!(json["comparison"]["last_local_height"], 7);
}

#[test]
fn end_height_falls_back_to_reference_maximum() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    // No local cursor: the end height has to come from the reference itself.
    write_database(&db, &seeds(0, 3, 1), None);
    write_reference(&reference, &seeds(0, 3, 1));

    let result = run(&args(&db, &reference, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_OK);

    let json = report(&output);
    assert_eq!(json["requested"]["end_height"], 3);
    assert_eq!(json["requested"]["end_height_source"], "reference_max");
    assert_eq!(
        json["local_cursor"]["local_root_index"],
        serde_json::Value::Null
    );
}

#[test]
fn database_bytes_and_sequence_number_are_unchanged() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    write_database(&db, &seeds(0, 5, 1), Some(5));
    write_reference(&reference, &seeds(0, 5, 1));

    // Measure only after the last writable handle is closed so the snapshot is
    // stable; every later write would show up in the comparison.
    let sequence_before = sequence_number(&db);
    let before = data_files(&db);

    let result = run(&args(&db, &reference, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_OK);

    let after = data_files(&db);
    assert_eq!(
        before.keys().collect::<Vec<_>>(),
        after.keys().collect::<Vec<_>>(),
        "the sweep must not create or remove database files"
    );
    assert_eq!(before, after, "the sweep must not rewrite database bytes");
    assert_eq!(
        sequence_number(&db),
        sequence_before,
        "the RocksDB sequence number must be unchanged"
    );
}

#[test]
fn secondary_mode_requires_scratch_path_and_leaves_primary_intact() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let secondary = tmp.path().join("secondary");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    write_database(&db, &seeds(0, 3, 1), Some(3));
    write_reference(&reference, &seeds(0, 3, 1));

    let mut missing_scratch = args(&db, &reference, &output);
    missing_scratch.mode = OpenMode::Secondary;
    let error = run(&missing_scratch).expect_err("secondary mode needs a scratch path");
    assert!(format!("{error:#}").contains("--secondary-path"));

    let mut inside_primary = args(&db, &reference, &output);
    inside_primary.mode = OpenMode::Secondary;
    inside_primary.secondary_path = Some(db.join("scratch"));
    let error = run(&inside_primary).expect_err("scratch inside the primary is refused");
    assert!(format!("{error:#}").contains("outside --db"));

    let before = data_files(&db);
    let mut run_args = args(&db, &reference, &output);
    run_args.mode = OpenMode::Secondary;
    run_args.secondary_path = Some(secondary.clone());
    let result = run(&run_args).expect("secondary sweep runs");
    assert_eq!(result.exit_code, EXIT_OK);
    assert_eq!(report(&output)["db"]["open_mode"], "secondary");
    assert_eq!(data_files(&db), before);
}

#[test]
fn duplicate_reference_rows_are_classified() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    write_database(&db, &seeds(0, 3, 1), Some(3));

    // Identical repeats are harmless.
    let identical = tmp.path().join("identical.jsonl");
    let mut lines: Vec<String> = seeds(0, 3, 1)
        .iter()
        .map(|(height, hash)| format!("{{\"height\": {height}, \"roothash\": \"{hash}\"}}"))
        .collect();
    lines.push(lines[2].clone());
    write_reference_lines(&identical, &lines);
    let output = tmp.path().join("identical.json");
    let result = run(&args(&db, &identical, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_OK);
    let json = report(&output);
    assert_eq!(json["reference"]["duplicate_identical_heights"], 1);
    assert_eq!(json["reference"]["duplicate_conflicting_heights"], 0);

    // Conflicting repeats mean the reference itself is untrustworthy.
    let conflicting = tmp.path().join("conflicting.jsonl");
    lines.push(format!(
        "{{\"height\": 2, \"roothash\": \"{}\"}}",
        hash_for(2, 8)
    ));
    write_reference_lines(&conflicting, &lines);
    let output = tmp.path().join("conflicting.json");
    let result = run(&args(&db, &conflicting, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_VERIFICATION_FAILED);
    let json = report(&output);
    assert_eq!(json["reference"]["duplicate_conflicting_heights"], 1);
    assert_eq!(json["reference"]["first_conflict_height"], 2);
}

#[test]
fn interleaved_repeat_is_still_a_conflict() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    write_database(&db, &seeds(0, 5, 1), Some(5));

    // `5, 4, 5` makes the repeat land exactly on the next slot of the pending
    // batch; it must still be read back instead of silently overwritten.
    let reference = tmp.path().join("interleaved.jsonl");
    write_reference_lines(
        &reference,
        &[
            format!("{{\"height\": 5, \"roothash\": \"{}\"}}", hash_for(5, 1)),
            format!("{{\"height\": 4, \"roothash\": \"{}\"}}", hash_for(4, 1)),
            format!("{{\"height\": 5, \"roothash\": \"{}\"}}", hash_for(5, 2)),
        ],
    );
    let output = tmp.path().join("result.json");
    let result = run(&args(&db, &reference, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_VERIFICATION_FAILED);
    let json = report(&output);
    assert_eq!(json["reference"]["duplicate_conflicting_heights"], 1);
    assert_eq!(json["reference"]["first_conflict_height"], 5);
    assert_eq!(json["reference"]["duplicate_identical_heights"], 0);
}

#[test]
fn undecodable_local_record_is_a_decode_failure() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    let reference = tmp.path().join("reference.jsonl");
    let output = tmp.path().join("result.json");

    write_database(&db, &seeds(0, 3, 1), Some(3));
    write_reference(&reference, &seeds(0, 3, 1));
    // Truncated value: 33 bytes is one short of a valid record.
    write_raw_value(
        &db,
        record_schema::state_root_record_key(2).to_array(),
        vec![0u8; 33],
    );

    let result = run(&args(&db, &reference, &output)).expect("sweep runs");
    assert_eq!(result.exit_code, EXIT_VERIFICATION_FAILED);
    let json = report(&output);
    assert_eq!(json["comparison"]["decode_errors"], 1);
    assert_eq!(json["comparison"]["compared_heights"], 3);
}

#[test]
#[ignore = "throughput probe for the 13.1M-row sizing estimate; run it explicitly"]
fn perf_probe_unordered_index_throughput() {
    use crate::reference_index::HeightHashIndex;

    // 500k unruly rows are enough to extrapolate the two dominant costs of a
    // 13.15M-row unordered reference: one seek+write per row and one seek+read
    // per comparison.
    const ROWS: u32 = 500_000;
    let tmp = fixture();
    let mut index = HeightHashIndex::create(tmp.path(), false).expect("create index");

    // Deterministic Fisher-Yates shuffle so the probe is repeatable.
    let mut order: Vec<u32> = (0..ROWS).collect();
    let mut state = 0x9e37_79b9_7f4a_7c15_u64;
    for i in (1..order.len()).rev() {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let j = (state % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }

    let insert_started = std::time::Instant::now();
    for height in &order {
        index
            .insert(*height, &hash_for(*height, 1).to_array())
            .expect("insert");
    }
    let insert_seconds = insert_started.elapsed().as_secs_f64();
    index.seal().expect("seal");

    let lookup_started = std::time::Instant::now();
    let mut found = 0_u32;
    for height in 0..ROWS {
        if index.get(height).expect("get").is_some() {
            found += 1;
        }
    }
    let lookup_seconds = lookup_started.elapsed().as_secs_f64();

    println!(
        "perf probe: rows={ROWS} insert={insert_seconds:.2}s ({:.0}/s) lookup={lookup_seconds:.2}s \
         ({:.0}/s) found={found} slot_bytes={}",
        ROWS as f64 / insert_seconds,
        ROWS as f64 / lookup_seconds,
        index.allocated_bytes(),
    );
    assert_eq!(found, ROWS);
    assert_eq!(index.stats().heights, u64::from(ROWS));
}

#[test]
fn cli_exit_codes_follow_the_verdict() {
    let tmp = fixture();
    let db = tmp.path().join("db");
    write_database(&db, &seeds(0, 3, 1), Some(3));

    let matching = tmp.path().join("matching.jsonl");
    write_reference(&matching, &seeds(0, 3, 1));
    assert_eq!(
        run_cli(args(&db, &matching, &tmp.path().join("ok.json"))),
        EXIT_OK,
        "a fully matching sweep exits 0"
    );

    let mut rows = seeds(0, 3, 1);
    rows[2].1 = hash_for(2, 6);
    let mismatched = tmp.path().join("mismatched.jsonl");
    write_reference(&mismatched, &rows);
    assert_eq!(
        run_cli(args(&db, &mismatched, &tmp.path().join("bad.json"))),
        EXIT_VERIFICATION_FAILED,
        "a mismatch exits 1"
    );

    let mut empty_db = args(Path::new(""), &matching, &tmp.path().join("usage.json"));
    empty_db.db = PathBuf::new();
    assert_eq!(
        run_cli(empty_db),
        EXIT_USAGE,
        "refused arguments exit 2 without opening anything"
    );
}

#[test]
fn no_mutation_options_or_empty_paths_are_accepted() {
    let parsed = Args::try_parse_from([
        "state-root-sweep",
        "--db",
        "db",
        "--reference",
        "reference.jsonl",
        "--output",
        "result.json",
        "--write",
    ]);
    assert!(parsed.is_err(), "a mutation flag must not exist");
    let flags = [
        "--repair",
        "--flush",
        "--compact",
        "--checkpoint",
        "--migrate",
        "--delete",
    ];
    for flag in flags {
        let parsed = Args::try_parse_from([
            "state-root-sweep",
            "--db",
            "db",
            "--reference",
            "reference.jsonl",
            "--output",
            "result.json",
            flag,
        ]);
        assert!(parsed.is_err(), "{flag} must not be accepted");
    }

    let parsed = Args::try_parse_from([
        "state-root-sweep",
        "--db",
        "db",
        "--reference",
        "reference.jsonl",
        "--output",
        "result.json",
    ])
    .expect("minimal invocation parses");
    assert!(
        parsed.stop_on_first_mismatch,
        "stop-on-first-mismatch defaults to true"
    );

    let tmp = fixture();
    let reference = tmp.path().join("reference.jsonl");
    write_reference(&reference, &seeds(0, 0, 1));
    let mut empty_db = args(Path::new(""), &reference, &tmp.path().join("result.json"));
    empty_db.db = PathBuf::new();
    let error = run(&empty_db).expect_err("an empty --db is refused");
    assert!(format!("{error:#}").contains("--db must not be empty"));
}
