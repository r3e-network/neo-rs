#![allow(dead_code)] // each integration-test binary includes this module but uses only part of it

use neo_core::network::p2p::payloads::{signer::Signer, witness::Witness};
use neo_core::{Transaction, UInt160, WitnessScope};
use std::path::{Path, PathBuf};

pub fn test_transaction() -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x01020304);
    tx.set_system_fee(100_000_000);
    tx.set_network_fee(1);
    tx.set_valid_until_block(0x01020304);
    tx.set_script(vec![0x11]);
    tx.set_signers(vec![Signer::new(
        UInt160::zero(),
        WitnessScope::CALLED_BY_ENTRY,
    )]);
    tx.set_attributes(Vec::new());
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

pub fn test_byte_array(size: usize, fill_byte: u8) -> Vec<u8> {
    vec![fill_byte; size]
}

/// Repository root, i.e. the directory that contains `neo-core/`.
pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("neo-core must live inside the repository root")
        .to_path_buf()
}

/// State-root layouts this repository has used, in probe order.
const STATE_ROOT_LAYOUTS: [&str; 3] = [
    "data/Plugins/StateService/StateRoot",
    "data/mainnet/StateRoot",
    "data/Plugins/mainnet/StateRoot",
];

/// Resolve the state-root RocksDB used by the `mainnet_block_*_repro` tests.
///
/// `NEO_REPRO_DB_PATH` wins when set. A relative override resolves against
/// [`repo_root`], never against the process working directory: `cargo test`
/// runs with `cwd = neo-core/`, so the documented `data/...` path used to be
/// read from `neo-core/data/...` and fail to open.
///
/// Without an override, [`STATE_ROOT_LAYOUTS`] is probed and the first existing
/// directory wins — the two layouts are both in use across documented setups,
/// so hard-coding one of them made half the reproducers unusable. When nothing
/// exists the first candidate is returned so the caller fails on an absolute
/// path it can act on, after the probe list is logged.
pub fn repro_state_root_path() -> PathBuf {
    let root = repo_root();
    if let Some(raw) = std::env::var("NEO_REPRO_DB_PATH")
        .ok()
        .filter(|path| !path.is_empty())
    {
        let path = Path::new(&raw);
        return if path.is_absolute() {
            path.to_path_buf()
        } else {
            root.join(path)
        };
    }

    let candidates: Vec<PathBuf> = STATE_ROOT_LAYOUTS
        .iter()
        .map(|rel| rel.split('/').fold(root.clone(), |acc, part| acc.join(part)))
        .collect();
    let found = candidates.iter().find(|path| path.is_dir()).cloned();
    match found {
        Some(path) => path,
        None => {
            eprintln!("[repro] no local state-root DB found, probed:");
            for path in &candidates {
                eprintln!("  - {}", path.display());
            }
            candidates
                .into_iter()
                .next()
                .expect("STATE_ROOT_LAYOUTS is non-empty")
        }
    }
}

/// Whether missing fixture data must fail instead of skipping.
///
/// The `mainnet_block_*_repro` tests cannot verify anything when the state-root
/// store does not cover the height they replay, and a bare `return` reports that
/// as `ok`. Local runs are expected to work against partial stores, so skipping
/// is the default; a validation job that claims to have run them sets
/// `NEO_REPRO_REQUIRE_DATA=1` and gets a hard failure instead.
pub fn repro_requires_data() -> bool {
    matches!(
        std::env::var("NEO_REPRO_REQUIRE_DATA").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE")
    )
}
