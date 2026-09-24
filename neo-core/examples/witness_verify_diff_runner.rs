//! Witness-verification differential runner (Rust side) on real MainNet blocks.
//!
//! Loads the real MainNet block fixtures (neo-core/tests/protocol_compliance/
//! test_vectors/mainnet_blocks.json, fetched from seed1.neo.org), deserializes
//! every block, and runs the FULL witness verification path
//! (`Helper::verify_witnesses` — script-hash resolution + verification scripts
//! in the VM + signature checks + gas accounting) on every transaction against
//! an empty in-memory snapshot.
//!
//! Outcomes are written per-transaction so the C# (Neo 3.10.1) runner can run
//! the equivalent `Neo.SmartContract.Helper.VerifyWitnesses` and the two
//! accept/reject sets can be compared. Absolute accepts may legitimately be
//! below 100% (account contracts whose verification reads storage are rejected
//! on an empty snapshot on BOTH sides — the differential compares outcome
//! equality, not absolute counts).
//!
//! Usage:
//!   cargo run -q -p neo-core --example witness_verify_diff_runner -- \
//!     <blocks.json> <out_rust.json> [max_gas_datoshi]

use neo_core::neo_io::{MemoryReader, Serializable};
use neo_core::network::p2p::payloads::{Block, Transaction};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::Helper;
use serde_json::{json, Value};
use std::collections::BTreeMap;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: witness_verify_diff_runner <blocks.json> <out.json> [max_gas_datoshi]");
        std::process::exit(2);
    }
    let raw = std::fs::read_to_string(&args[1]).unwrap();
    let fixture: Value = serde_json::from_str(&raw).unwrap();
    let magic = fixture["magic"].as_u64().unwrap() as u32;

    // Same gas budget on both sides; cap at Helper's own MAX like the C# side.
    let requested = args
        .get(3)
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(10_000_000_000); // 100 GAS, generous
    let max_gas = requested.min(150_000_000);

    let settings = ProtocolSettings {
        network: magic,
        ..ProtocolSettings::default()
    };
    let snapshot = DataCache::new(false);

    let mut outcomes: BTreeMap<String, Value> = BTreeMap::new();
    let mut total = 0usize;
    let mut accepted = 0usize;
    let mut errors = 0usize;

    for block_json in fixture["blocks"].as_array().unwrap() {
        let hex_str = block_json["block_hex"].as_str().unwrap();
        let bytes: Vec<u8> = (0..hex_str.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16).unwrap())
            .collect();
        let mut reader = MemoryReader::new(&bytes);
        let block = match Block::deserialize(&mut reader) {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "height {}: block decode failed: {e:?}",
                    block_json["height"]
                );
                errors += 1;
                continue;
            }
        };
        for tx in &block.transactions {
            total += 1;
            let key = tx.hash().to_hex_string();
            let ok = Helper::verify_witnesses(tx, &settings, &snapshot, max_gas);
            if ok {
                accepted += 1;
            }
            outcomes.insert(
                key.clone(),
                json!({
                    "tx_hash": key,
                    "height": block_json["height"],
                    "rust_verify": ok,
                }),
            );
        }
    }

    let results: Vec<&Value> = outcomes.values().collect();
    let output = json!({
        "magic": magic,
        "max_gas_datoshi": max_gas,
        "total_transactions": total,
        "accepted": accepted,
        "rejected": total - accepted,
        "decode_errors": errors,
        "results": results,
    });
    serde_json::to_writer_pretty(std::fs::File::create(&args[2]).unwrap(), &output).unwrap();
    println!(
        "witness_verify_diff_runner: {accepted}/{total} accepted on empty snapshot, {errors} decode errors"
    );
}
