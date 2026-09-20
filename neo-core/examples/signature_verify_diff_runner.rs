//! Signature-primitive differential runner (Rust side) on real MainNet blocks.
//!
//! For every transaction in the MainNet fixture it:
//!   1. deserializes the block/transactions,
//!   2. computes sign_data = network(magic, 4 LE) || tx_hash(32),
//!   3. for each witness, extracts the ECDSA signature from the invocation
//!      script (PUSHDATA1 64) and the public key from the verification script
//!      (PUSHDATA1 33), then runs secp256r1 verification,
//!   4. records the per-witness verify result and, when the witness shape is
//!      a plain signature contract, the pubkey/signature bytes for the C# side
//!      to run the equivalent secp256r1 verify.
//!
//! The C# (Neo 3.10.1) runner re-verifies each recorded signature and the two
//! verify outcomes are compared. Only standard signature-contract witnesses are
//! compared this way; other shapes are recorded but not signature-checked.
//!
//! Usage:
//!   cargo run -q -p neo-core --example signature_verify_diff_runner -- \
//!     <blocks.json> <out_rust.json>

use neo_core::cryptography::Secp256r1Crypto;
use neo_core::network::p2p::payloads::Block;
use neo_core::network::p2p::helper::get_sign_data;
use neo_core::neo_io::{MemoryReader, Serializable};
use serde_json::{json, Value};
use std::collections::BTreeMap;

const PUSH1: u8 = 0x0c;
const SYSCALL_CHECKSIG: &[u8] = &[0x41, 0x56, 0xe7, 0xb3, 0x27];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: signature_verify_diff_runner <blocks.json> <out.json>");
        std::process::exit(2);
    }
    let raw = std::fs::read_to_string(&args[1]).unwrap();
    let fixture: Value = serde_json::from_str(&raw).unwrap();
    let magic = fixture["magic"].as_u64().unwrap() as u32;

    let mut outcomes: BTreeMap<String, Value> = BTreeMap::new();
    let mut total_sig_witnesses = 0usize;
    let mut rust_verify_ok = 0usize;

    for block_json in fixture["blocks"].as_array().unwrap() {
        let hex_str = block_json["block_hex"].as_str().unwrap();
        let bytes: Vec<u8> = (0..hex_str.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&hex_str[i..i + 2], 16).unwrap())
            .collect();
        let mut reader = MemoryReader::new(&bytes);
        let block: Block = Block::deserialize(&mut reader).unwrap();

        for tx in &block.transactions {
            let tx_hash = tx.hash();
            let sign_data = match get_sign_data(tx, magic) {
                Ok(sd) => sd,
                Err(e) => {
                    outcomes.insert(
                        tx_hash.to_hex_string(),
                        json!({ "error": format!("get_sign_data: {e:?}") }),
                    );
                    continue;
                }
            };
            let wits = tx.witnesses();
            let mut wit_entries = Vec::new();
            for wit in wits {
                let inv = wit.invocation_script();
                let ver = wit.verification_script();
                // Standard signature contract: PUSHDATA1 64 sig / PUSHDATA1 33 pub + CheckSig.
                let is_sig = inv.len() == 66
                    && inv[0] == PUSH1
                    && ver.len() == 40
                    && ver[0] == PUSH1
                    && ver[35..] == SYSCALL_CHECKSIG[..];
                if is_sig {
                    let sig = &inv[2..66];
                    let pubkey = &ver[2..35];
                    let ok = Secp256r1Crypto::verify(&sign_data, sig.try_into().unwrap(), pubkey)
                        .unwrap_or(false);
                    total_sig_witnesses += 1;
                    if ok {
                        rust_verify_ok += 1;
                    }
                    wit_entries.push(json!({
                        "kind": "signature",
                        "signature": hex(sig),
                        "pubkey": hex(pubkey),
                        "rust_verify": ok,
                    }));
                } else {
                    wit_entries.push(json!({
                        "kind": "other",
                        "inv_len": inv.len(),
                        "ver_len": ver.len(),
                    }));
                }
            }
            outcomes.insert(
                tx_hash.to_hex_string(),
                json!({
                    "tx_hash": tx_hash.to_hex_string(),
                    "height": block_json["height"],
                    "magic": magic,
                    "sign_data": hex(&sign_data),
                    "witnesses": wit_entries,
                }),
            );
        }
    }

    let results: Vec<&Value> = outcomes.values().collect();
    let output = json!({
        "magic": magic,
        "total_signature_witnesses": total_sig_witnesses,
        "rust_verify_ok": rust_verify_ok,
        "results": results,
    });
    serde_json::to_writer_pretty(std::fs::File::create(&args[2]).unwrap(), &output).unwrap();
    println!(
        "signature_verify_diff_runner: {rust_verify_ok}/{total_sig_witnesses} signature witnesses verify on secp256r1"
    );
}

fn hex(v: &[u8]) -> String {
    v.iter().map(|b| format!("{b:02x}")).collect()
}
