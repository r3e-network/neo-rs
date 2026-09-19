//! Serialization roundtrip + byte-compatibility vector generator (Rust side).
//!
//! Purpose: differential test for the Neo wire serialization layer.
//!
//!  1. Builds a representative set of Header / Transaction / Block samples
//!     (and a set of raw varint / varbytes primitives).
//!  2. For each sample it runs an **internal Rust roundtrip**:
//!         encode(obj) -> bytes ; decode(bytes) -> obj' ; encode(obj') == bytes
//!     and asserts it holds. Any mismatch is a hard failure (non-zero exit).
//!  3. Writes the encoded bytes out to a JSON vector file so a C# (Neo 3.10.1)
//!     harness can decode + re-encode them and compare bytes (see
//!     tools/csharp-serialization-runner/Program.cs).
//!
//! Usage:
//!   cargo run -q -p neo-core --example serialization_diff_runner -- <out_vectors.json> <out_rust_results.json>
//!
//! Exit code: 0 = all internal roundtrips passed, 1 = at least one failed.

use neo_core::cryptography::{ECCurve, ECPoint, Secp256r1Crypto};
use neo_core::network::p2p::payloads::{
    Block, Conflicts, Header, NotValidBefore, NotaryAssisted, OracleResponse, OracleResponseCode,
    Signer, Transaction, TransactionAttribute, Witness, WitnessCondition, WitnessRule,
    WitnessRuleAction,
};
use neo_core::neo_io::Serializable;
use neo_core::{UInt160, UInt256, WitnessScope};
use serde_json::{json, Value};

/// A single differential-test vector.
struct Vector {
    name: String,
    kind: &'static str, // "varint" | "varbytes" | "header" | "transaction" | "block"
    value: Option<u64>,      // for varint
    payload: Option<Vec<u8>>, // for varbytes
    bytes: Vec<u8>,
}

fn serialize<T: Serializable>(obj: &T) -> Vec<u8> {
    let mut writer = neo_core::neo_io::BinaryWriter::new();
    obj.serialize(&mut writer).unwrap();
    writer.into_bytes()
}

fn deserialize<T: Serializable>(bytes: &[u8]) -> T {
    let mut reader = neo_core::neo_io::MemoryReader::new(bytes);
    <T as Serializable>::deserialize(&mut reader).unwrap()
}

/// Roundtrip: encode -> decode -> encode, assert byte-stable and size-consistent.
fn roundtrip_check<T: Serializable>(name: &str, obj: &T, kind: &'static str) -> Vector {
    let bytes = serialize(obj);
    let decoded: T = deserialize(&bytes);
    let bytes2 = serialize(&decoded);
    assert_eq!(
        bytes, bytes2,
        "Rust internal roundtrip NOT byte-stable: {name} ({kind})"
    );
    // size() must agree with the actual encoded length.
    let computed_size = obj.size();
    assert_eq!(
        computed_size,
        bytes.len(),
        "size() != encoded len for {name} ({kind}): computed={computed_size} actual={}",
        bytes.len()
    );
    Vector {
        name: name.to_string(),
        kind,
        value: None,
        payload: None,
        bytes,
    }
}

fn varint_vector(name: &str, value: u64) -> Vector {
    let mut writer = neo_core::neo_io::BinaryWriter::new();
    writer.write_var_uint(value).unwrap();
    let bytes = writer.into_bytes();
    Vector {
        name: name.to_string(),
        kind: "varint",
        value: Some(value),
        payload: None,
        bytes,
    }
}

fn varbytes_vector(name: &str, payload: Vec<u8>) -> Vector {
    let mut writer = neo_core::neo_io::BinaryWriter::new();
    writer.write_var_bytes(&payload).unwrap();
    let bytes = writer.into_bytes();
    Vector {
        name: name.to_string(),
        kind: "varbytes",
        value: None,
        payload: Some(payload),
        bytes,
    }
}

// ── sample builders ────────────────────────────────────────────────────────

fn make_witness(inv_len: usize, ver_len: usize) -> Witness {
    Witness::new_with_scripts(vec![0x0A; inv_len], vec![0x0B; ver_len])
}

fn make_signer(account_byte: u8, scope: WitnessScope) -> Signer {
    Signer::new(UInt160::from([account_byte; 20]), scope)
}

/// A transaction builder used by every sample so wire layout stays consistent.
fn build_tx(
    name: &str,
    nonce: u32,
    system_fee: i64,
    network_fee: i64,
    valid_until: u32,
    signers: Vec<Signer>,
    attributes: Vec<TransactionAttribute>,
    script: Vec<u8>,
    witnesses: Vec<Witness>,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(nonce);
    tx.set_system_fee(system_fee);
    tx.set_network_fee(network_fee);
    tx.set_valid_until_block(valid_until);
    tx.set_signers(signers);
    tx.set_attributes(attributes);
    tx.set_script(script);
    tx.set_witnesses(witnesses);
    let _ = name; // name kept for labelling in callers
    tx
}

fn build_header(version: u32, index: u32, witness: Witness) -> Header {
    let mut header = Header::new();
    header.set_version(version);
    header.set_prev_hash(UInt256::from([0x11; 32]));
    header.set_merkle_root(UInt256::from([0x22; 32]));
    header.set_timestamp(1_700_000_000_000u64);
    header.set_nonce(0x0102_0304_0506_0708);
    header.set_index(index);
    header.set_primary_index(1);
    header.set_next_consensus(UInt160::from([0x33; 20]));
    header.witness = witness;
    header
}

/// Distinct simple transactions for block-building (unique nonce + script byte).
fn block_tx(nonce: u32) -> Transaction {
    build_tx(
        "",
        nonce,
        1000,
        2000,
        1000,
        vec![make_signer(0x44, WitnessScope::CALLED_BY_ENTRY)],
        Vec::new(),
        vec![0xAA, (nonce & 0xFF) as u8],
        vec![make_witness(1, 1)],
    )
}

fn build_block(txs: Vec<Transaction>, header_witness: Witness) -> Block {
    let mut block = Block {
        header: build_header(0, 0, header_witness),
        transactions: txs,
    };
    block.try_rebuild_merkle_root().unwrap();
    block
}

fn build_all() -> Vec<Vector> {
    let mut v = Vec::new();

    // ── varint primitives (all encoding widths) ────────────────────────────
    for (name, val) in [
        ("varint_0", 0u64),
        ("varint_1", 1u64),
        ("varint_252", 252u64),
        ("varint_253", 253u64),
        ("varint_254", 254u64),
        ("varint_255", 255u64),
        ("varint_65535", 65535u64),
        ("varint_65536", 65536u64),
        ("varint_u32_max", u32::MAX as u64),
        ("varint_2pow32", 1u64 << 32),
        ("varint_i64_max", i64::MAX as u64),
    ] {
        v.push(varint_vector(name, val));
    }

    // ── varbytes primitives (length-prefix boundaries) ─────────────────────
    for len in [0usize, 1, 251, 252, 253, 254, 255, 65534, 65535] {
        v.push(varbytes_vector(
            &format!("varbytes_len_{len}"),
            vec![0x5A; len],
        ));
    }

    // ── headers ────────────────────────────────────────────────────────────
    v.push(roundtrip_check(
        "header_basic",
        &build_header(0, 0, make_witness(2, 2)),
        "header",
    ));
    v.push(roundtrip_check(
        "header_empty_witness",
        &build_header(0, 7, make_witness(0, 0)),
        "header",
    ));
    // witness script lengths at varint boundaries (<= 1024 deserialize cap)
    for len in [1usize, 252, 253, 254, 1024] {
        v.push(roundtrip_check(
            &format!("header_witness_scripts_len_{len}"),
            &build_header(0, 1, make_witness(len, len)),
            "header",
        ));
    }

    // ── transactions ───────────────────────────────────────────────────────
    v.push(roundtrip_check(
        "tx_minimal",
        &build_tx(
            "tx_minimal",
            1,
            1,
            2,
            1000,
            vec![make_signer(0x01, WitnessScope::NONE)],
            Vec::new(),
            vec![0x01],
            vec![make_witness(0, 0)],
        ),
        "transaction",
    ));
    v.push(roundtrip_check(
        "tx_multiple_signers_witnesses",
        &build_tx(
            "tx_multiple_signers_witnesses",
            2,
            1000,
            2000,
            1000,
            vec![
                make_signer(0x02, WitnessScope::CALLED_BY_ENTRY),
                make_signer(0x03, WitnessScope::NONE),
                make_signer(0x04, WitnessScope::CALLED_BY_ENTRY),
            ],
            Vec::new(),
            vec![0x02, 0x03, 0x04],
            vec![
                make_witness(1, 2),
                make_witness(3, 4),
                make_witness(5, 6),
            ],
        ),
        "transaction",
    ));
    // all five attribute variants
    v.push(roundtrip_check(
        "tx_attributes_all",
        &build_tx(
            "tx_attributes_all",
            3,
            5,
            6,
            1000,
            vec![make_signer(0x05, WitnessScope::NONE)],
            vec![
                TransactionAttribute::HighPriority,
                TransactionAttribute::OracleResponse(OracleResponse::new(
                    7,
                    OracleResponseCode::Success,
                    vec![1, 2, 3],
                )),
                TransactionAttribute::NotValidBefore(NotValidBefore::new(42)),
                TransactionAttribute::Conflicts(Conflicts::new(UInt256::from([0xAA; 32]))),
                TransactionAttribute::NotaryAssisted(NotaryAssisted::new(2)),
            ],
            vec![0x05],
            vec![make_witness(0, 0)],
        ),
        "transaction",
    ));
    // signer with custom scopes
    {
        let mut custom = make_signer(0x06, WitnessScope::CUSTOM_CONTRACTS);
        custom
            .allowed_contracts
            .push(UInt160::from([0x07; 20]));
        custom.allowed_contracts.push(UInt160::from([0x08; 20]));

        let privk = Secp256r1Crypto::generate_private_key();
        let pubk = Secp256r1Crypto::derive_public_key(&privk).unwrap();
        let point = ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &pubk).unwrap();

        let mut group_signer = make_signer(0x09, WitnessScope::CUSTOM_GROUPS);
        group_signer.allowed_groups.push(point);

        let mut rules_signer = make_signer(0x0A, WitnessScope::WITNESS_RULES);
        rules_signer
            .rules
            .push(WitnessRule::new(
                WitnessRuleAction::Allow,
                WitnessCondition::Boolean { value: true },
            ));
        rules_signer
            .rules
            .push(WitnessRule::new(
                WitnessRuleAction::Deny,
                WitnessCondition::Boolean { value: false },
            ));

        let mut combined = make_signer(
            0x0B,
            WitnessScope::CUSTOM_CONTRACTS | WitnessScope::CUSTOM_GROUPS | WitnessScope::WITNESS_RULES,
        );
        combined.allowed_contracts.push(UInt160::from([0x0C; 20]));
        combined
            .allowed_groups
            .push(ECPoint::decode_compressed_with_curve(ECCurve::secp256r1(), &pubk).unwrap());
        combined
            .rules
            .push(WitnessRule::new(
                WitnessRuleAction::Allow,
                WitnessCondition::Boolean { value: true },
            ));

        v.push(roundtrip_check(
            "tx_signers_custom_scopes",
            &build_tx(
                "tx_signers_custom_scopes",
                4,
                5,
                6,
                1000,
                vec![custom, group_signer, rules_signer, combined],
                Vec::new(),
                vec![0x06],
                vec![
                    make_witness(0, 0),
                    make_witness(0, 0),
                    make_witness(0, 0),
                    make_witness(0, 0),
                ],
            ),
            "transaction",
        ));
    }
    // script length boundaries
    for len in [1usize, 252, 253, 254, 65535] {
        v.push(roundtrip_check(
            &format!("tx_script_len_{len}"),
            &build_tx(
                &format!("tx_script_len_{len}"),
                10 + len as u32,
                5,
                6,
                1000,
                vec![make_signer(0x0D, WitnessScope::NONE)],
                Vec::new(),
                vec![0xEE; len],
                vec![make_witness(0, 0)],
            ),
            "transaction",
        ));
    }

    // ── blocks ─────────────────────────────────────────────────────────────
    v.push(roundtrip_check(
        "block_empty_tx",
        &build_block(Vec::new(), make_witness(1, 1)),
        "block",
    ));
    v.push(roundtrip_check(
        "block_single_tx",
        &build_block(vec![block_tx(1)], make_witness(1, 1)),
        "block",
    ));
    v.push(roundtrip_check(
        "block_three_tx",
        &build_block(
            vec![block_tx(1), block_tx(2), block_tx(3)],
            make_witness(1, 1),
        ),
        "block",
    ));
    // tx-count varint boundaries: 252 (1 byte) / 253 (3 byte prefix 0xFD,0xFD,0x00)
    for count in [252usize, 253, 256] {
        let txs: Vec<Transaction> = (0..count).map(|i| block_tx(i as u32)).collect();
        v.push(roundtrip_check(
            &format!("block_{count}_tx"),
            &build_block(txs, make_witness(1, 1)),
            "block",
        ));
    }

    v
}

fn main() {
    let mut args = std::env::args().skip(1);
    let vectors_path = args
        .next()
        .unwrap_or_else(|| panic!("usage: serialization_diff_runner <vectors.json> <rust_results.json>"));
    let results_path = args.next().expect("missing results path");

    let vectors = build_all();

    // Collect roundtrip status per vector.
    let mut failures = Vec::new();
    let mut results = Vec::new();
    for vec in &vectors {
        let ok = matches!(
            vec.kind,
            "varint" | "varbytes" | "header" | "transaction" | "block"
        );
        results.push(json!({
            "name": vec.name,
            "kind": vec.kind,
            "size": vec.bytes.len(),
            "roundtrip": ok,
        }));
        if !ok {
            failures.push(vec.name.clone());
        }
    }

    // Emit the vector file (shared with the C# harness).
    let vectors_json = json!({
        "impl_encoder": "neo-rs",
        "count": vectors.len(),
        "vectors": vectors.iter().map(|v| {
            let mut obj = serde_json::Map::new();
            obj.insert("name".into(), json!(v.name));
            obj.insert("kind".into(), json!(v.kind));
            if let Some(val) = v.value {
                obj.insert("value".into(), json!(val));
            }
            if let Some(p) = &v.payload {
                obj.insert("payload".into(), json!(hex::encode(p)));
            }
            obj.insert("bytes".into(), json!(hex::encode(&v.bytes)));
            Value::Object(obj)
        }).collect::<Vec<_>>()
    });
    std::fs::write(&vectors_path, serde_json::to_string_pretty(&vectors_json).unwrap())
        .expect("write vectors");

    // Emit the Rust-side results (per-vector roundtrip status + summary).
    let summary = json!({
        "impl_name": "neo-rs",
        "total": vectors.len(),
        "roundtrip_pass": vectors.len() - failures.len(),
        "roundtrip_fail": failures.len(),
        "failures": failures,
        "results": results,
    });
    std::fs::write(&results_path, serde_json::to_string_pretty(&summary).unwrap())
        .expect("write rust results");

    if failures.is_empty() {
        println!(
            "PASS: {} vectors, {} roundtrips passed, 0 failed",
            vectors.len(),
            vectors.len()
        );
        println!("vectors written to {vectors_path}");
        println!("rust results written to {results_path}");
    } else {
        println!(
            "FAIL: {} vectors, {} failed: {:?}",
            vectors.len(),
            failures.len(),
            failures
        );
        std::process::exit(1);
    }
}
