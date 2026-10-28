#![cfg(feature = "rocksdb")]

// Bulk compat test: for every mainnet height present in the local
// data/mainnet/StateRoot DB up to MAX_CLEAN_HEIGHT that also has a reference
// root in data/reference_stateroots.jsonl, assert the locally-stored state
// root matches the C# canonical root byte-for-byte.
//
// This covers the entire clean range of synced heights, not just the four
// hand-picked reproducer blocks. A single mismatch fails the test.
//
// Why MAX_CLEAN_HEIGHT exists: heights ≥172,613 in the current local DB are
// known stale, produced by a past buggy sync (see mainnet_block_172613_repro
// — current code at that block produces the correct C# root, but the stored
// DB value still reflects the old buggy run). Until the DB is re-synced from
// height 172,613 with current code, the bulk test must cap at 172,612 to
// distinguish real code regressions from known-stale stored data. Set
// `NEO_BULK_ROOT_MAX_HEIGHT` env var to override (e.g. raise after re-sync).
//
// `NEO_BULK_ROOT_DB_PATH` env var overrides the DB path. Useful for testing
// alternate backups, e.g.
//   NEO_BULK_ROOT_DB_PATH=data/mainnet.pre-274157-fix-20260419/StateRoot \
//   NEO_BULK_ROOT_MAX_HEIGHT=257982 \
//   NEO_BULK_ROOT_SKIPLIST=203262 \
//   cargo test ... mainnet_state_roots_vs_csharp -- --ignored --nocapture
//
// `NEO_BULK_ROOT_SKIPLIST` accepts a comma-separated list of heights to skip
// (known bad in the chosen DB but not relevant to current code correctness).

const MAX_CLEAN_HEIGHT_DEFAULT: u32 = 172_612;

use neo_core::UInt256;
use neo_core::persistence::StorageConfig;
use neo_core::persistence::{providers::RocksDBStoreProvider, store_provider::StoreProvider};
use neo_core::state_service::state_store::{
    SnapshotBackedStateStoreBackend, StateServiceSettings, StateStore,
};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::Arc;

fn read_env(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotPresent) => None,
        Err(std::env::VarError::NotUnicode(_)) => panic!("{name} must be valid Unicode"),
    }
}

fn parse_max_height(value: Option<&str>) -> Result<u32, String> {
    match value {
        Some(value) => value
            .parse::<u32>()
            .map_err(|error| format!("invalid NEO_BULK_ROOT_MAX_HEIGHT {value:?}: {error}")),
        None => Ok(MAX_CLEAN_HEIGHT_DEFAULT),
    }
}

fn parse_skiplist(value: Option<&str>) -> Result<HashSet<u32>, String> {
    let Some(value) = value else {
        return Ok(HashSet::new());
    };
    if value.trim().is_empty() {
        return Ok(HashSet::new());
    }

    value
        .split(',')
        .map(|part| {
            let part = part.trim();
            if part.is_empty() {
                return Err("NEO_BULK_ROOT_SKIPLIST contains an empty height".to_string());
            }
            part.parse::<u32>()
                .map_err(|error| format!("invalid NEO_BULK_ROOT_SKIPLIST height {part:?}: {error}"))
        })
        .collect()
}

fn expected_coverage(max_height: u32, skiplist: &HashSet<u32>) -> u64 {
    let skipped = skiplist
        .iter()
        .filter(|height| **height <= max_height)
        .count() as u64;
    u64::from(max_height) + 1 - skipped
}

struct ReferenceRoots {
    roots: BTreeMap<u32, UInt256>,
    skipped_above_cap: u64,
}

fn parse_reference_roots(reader: impl BufRead, max_height: u32) -> Result<ReferenceRoots, String> {
    let mut roots = BTreeMap::new();
    let mut seen_heights = HashSet::new();
    let mut skipped_above_cap = 0u64;

    for (index, line) in reader.lines().enumerate() {
        let line_number = index + 1;
        let line = line.map_err(|error| format!("read line {line_number}: {error}"))?;
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(&line)
            .map_err(|error| format!("line {line_number}: malformed JSON: {error}"))?;
        let height = value
            .get("height")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| format!("line {line_number}: missing or invalid height"))?;
        let height = u32::try_from(height)
            .map_err(|_| format!("line {line_number}: height {height} exceeds u32"))?;
        let reference = value
            .get("roothash")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                format!("line {line_number} (height {height}): missing or invalid roothash")
            })?;
        let reference_root = UInt256::parse(reference).map_err(|error| {
            format!("line {line_number} (height {height}): invalid roothash: {error:?}")
        })?;

        if !seen_heights.insert(height) {
            return Err(format!(
                "line {line_number}: duplicate reference height {height}"
            ));
        }
        if height > max_height {
            skipped_above_cap += 1;
        } else {
            roots.insert(height, reference_root);
        }
    }

    Ok(ReferenceRoots {
        roots,
        skipped_above_cap,
    })
}

fn open_state_store(state_root_path: &Path) -> StateStore {
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: state_root_path.to_path_buf(),
        read_only: true,
        ..StorageConfig::default()
    });
    let state_db = provider.get_store("").expect("open state root store");
    let backend = Arc::new(SnapshotBackedStateStoreBackend::new(state_db));
    StateStore::new(
        backend,
        StateServiceSettings {
            full_state: true,
            path: state_root_path.to_string_lossy().to_string(),
            ..StateServiceSettings::default()
        },
    )
}

#[test]
#[ignore = "requires data/mainnet/StateRoot and data/reference_stateroots.jsonl"]
fn local_state_roots_match_csharp_reference() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf();
    // Allow overriding the local state-root DB to test alternate backups
    // (e.g. NEO_BULK_ROOT_DB_PATH=data/mainnet.pre-274157-fix-20260419/StateRoot).
    //
    // Relative overrides are resolved against the repo root, not the process
    // working directory: `cargo test` runs with cwd=neo-core/, so a documented
    // `data/Plugins/...` path would otherwise point at neo-core/data/... and be
    // skipped without anyone noticing.
    let strict = matches!(
        read_env("NEO_BULK_ROOT_REQUIRE_DATA").as_deref(),
        Some("1" | "true" | "TRUE")
    );
    let explicit_path = read_env("NEO_BULK_ROOT_DB_PATH");
    if explicit_path.as_deref() == Some("") {
        panic!("NEO_BULK_ROOT_DB_PATH must not be empty");
    }
    let state_root_path = match explicit_path.as_deref() {
        Some(p) => {
            let path = Path::new(p);
            if path.is_absolute() {
                path.to_path_buf()
            } else {
                repo_root.join(path)
            }
        }
        None => repo_root.join("data/mainnet/StateRoot"),
    };
    let reference_path = repo_root.join("data/reference_stateroots.jsonl");
    let max_height = parse_max_height(read_env("NEO_BULK_ROOT_MAX_HEIGHT").as_deref())
        .unwrap_or_else(|error| panic!("{error}"));
    let skiplist = parse_skiplist(read_env("NEO_BULK_ROOT_SKIPLIST").as_deref())
        .unwrap_or_else(|error| panic!("{error}"));

    if !state_root_path.exists() {
        // An explicit override that does not exist is a caller error, not a
        // missing-data skip: silently returning would report `ok` while
        // comparing nothing.
        assert!(
            explicit_path.is_none() && !strict,
            "required state-root DB {} does not exist",
            state_root_path.display()
        );
        eprintln!(
            "[SKIPPED] {}: {} not present",
            module_path!(),
            state_root_path.display()
        );
        return;
    }
    if !reference_path.exists() {
        assert!(
            !strict,
            "required reference data {} does not exist",
            reference_path.display()
        );
        eprintln!(
            "[SKIPPED] {}: {} not present",
            module_path!(),
            reference_path.display()
        );
        return;
    }

    let store = open_state_store(&state_root_path);
    let file = File::open(&reference_path).expect("open reference jsonl");
    let references =
        parse_reference_roots(BufReader::new(file), max_height).unwrap_or_else(|error| {
            panic!(
                "invalid reference data {}: {error}",
                reference_path.display()
            )
        });

    let mut compared = 0u64;
    let mut skipped_no_local = 0u64;
    let mut skipped_explicit = 0u64;
    let mut missing_references = Vec::new();
    let mut missing_local_roots = Vec::new();
    let mut mismatches: Vec<(u32, UInt256, UInt256)> = Vec::new();

    for height in 0..=max_height {
        if skiplist.contains(&height) {
            if references.roots.contains_key(&height) {
                skipped_explicit += 1;
            }
            continue;
        }
        let Some(&reference_root) = references.roots.get(&height) else {
            if strict {
                missing_references.push(height);
            }
            continue;
        };
        let local_root = match store.get_state_root(height) {
            Some(root) => root.root_hash,
            None => {
                skipped_no_local += 1;
                if strict {
                    missing_local_roots.push(height);
                }
                continue;
            }
        };
        compared += 1;
        if local_root != reference_root {
            mismatches.push((height, local_root, reference_root));
            if mismatches.len() <= 10 {
                eprintln!(
                    "MISMATCH height={} local={} reference={}",
                    height, local_root, reference_root,
                );
            }
        }
    }
    eprintln!(
        "strict={} max_height_cap={} compared={} expected_compared={} skipped_no_local={} skipped_above_cap={} skipped_explicit={} missing_references={} missing_local_roots={} mismatches={}",
        strict,
        max_height,
        compared,
        expected_coverage(max_height, &skiplist),
        skipped_no_local,
        references.skipped_above_cap,
        skipped_explicit,
        missing_references.len(),
        missing_local_roots.len(),
        mismatches.len(),
    );

    if strict {
        assert!(
            missing_references.is_empty(),
            "strict coverage missing {} reference root(s); first heights: {:?}",
            missing_references.len(),
            &missing_references[..missing_references.len().min(10)],
        );
        assert!(
            missing_local_roots.is_empty(),
            "strict coverage missing {} local root(s); first heights: {:?}",
            missing_local_roots.len(),
            &missing_local_roots[..missing_local_roots.len().min(10)],
        );
        assert_eq!(
            compared,
            expected_coverage(max_height, &skiplist),
            "strict coverage did not compare every requested height",
        );
    } else {
        assert!(
            compared > 0,
            "no local heights found in reference set — check data layout",
        );
    }
    assert!(
        mismatches.is_empty(),
        "{} state-root mismatch(es) vs C# reference (first up to 10 printed above); first mismatch: height={} local={} reference={}",
        mismatches.len(),
        mismatches[0].0,
        mismatches[0].1,
        mismatches[0].2,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    const ROOT: &str = "0x0000000000000000000000000000000000000000000000000000000000000000";

    #[test]
    fn max_height_and_skiplist_values_are_strictly_parsed() {
        assert_eq!(parse_max_height(None), Ok(MAX_CLEAN_HEIGHT_DEFAULT));
        assert_eq!(parse_max_height(Some("172612")), Ok(172_612));
        assert!(parse_max_height(Some("")).is_err());
        assert!(parse_max_height(Some("invalid")).is_err());
        assert!(parse_max_height(Some("4294967296")).is_err());

        assert_eq!(parse_skiplist(Some("1, 2,3")), Ok(HashSet::from([1, 2, 3])));
        assert_eq!(parse_skiplist(Some("")), Ok(HashSet::new()));
        assert!(parse_skiplist(Some("1,invalid")).is_err());
        assert!(parse_skiplist(Some("1,,2")).is_err());
        assert!(parse_skiplist(Some("4294967296")).is_err());
    }

    #[test]
    fn reference_parser_accepts_valid_rows_and_counts_rows_above_cap() {
        let input = format!(
            "{{\"height\":0,\"roothash\":\"{ROOT}\"}}\n\n{{\"height\":2,\"roothash\":\"{ROOT}\"}}\n"
        );
        let references = parse_reference_roots(input.as_bytes(), 1).expect("valid references");

        assert_eq!(references.roots.len(), 1);
        assert!(references.roots.contains_key(&0));
        assert_eq!(references.skipped_above_cap, 1);
    }

    #[test]
    fn reference_parser_rejects_malformed_or_invalid_rows() {
        let cases = vec![
            ("not JSON\n".to_string(), "malformed JSON"),
            (format!("{{\"roothash\":\"{ROOT}\"}}\n"), "invalid height"),
            (
                format!("{{\"height\":-1,\"roothash\":\"{ROOT}\"}}\n"),
                "invalid height",
            ),
            ("{\"height\":0}\n".to_string(), "invalid roothash"),
            (
                "{\"height\":0,\"roothash\":12}\n".to_string(),
                "invalid roothash",
            ),
            (
                "{\"height\":0,\"roothash\":\"bad\"}\n".to_string(),
                "invalid roothash",
            ),
            (
                format!("{{\"height\":4294967296,\"roothash\":\"{ROOT}\"}}\n"),
                "exceeds u32",
            ),
        ];

        for (input, expected_error) in cases {
            let error = parse_reference_roots(input.as_bytes(), u32::MAX)
                .err()
                .expect("invalid reference row must fail");
            assert!(
                error.contains(expected_error),
                "expected {expected_error:?} in {error:?}"
            );
        }
    }

    #[test]
    fn reference_parser_rejects_duplicate_heights_even_above_cap() {
        let input = format!(
            "{{\"height\":2,\"roothash\":\"{ROOT}\"}}\n{{\"height\":2,\"roothash\":\"{ROOT}\"}}\n"
        );
        let error = parse_reference_roots(input.as_bytes(), 1)
            .err()
            .expect("duplicate height must fail");
        assert!(error.contains("duplicate reference height 2"));
    }

    #[test]
    fn expected_coverage_counts_every_unskipped_height() {
        assert_eq!(expected_coverage(2, &HashSet::new()), 3);
        assert_eq!(expected_coverage(2, &HashSet::from([1, 9])), 2);
        assert_eq!(
            expected_coverage(u32::MAX, &HashSet::from([u32::MAX])),
            u64::from(u32::MAX)
        );
    }
}
