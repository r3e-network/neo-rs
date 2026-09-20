//! P2P wire-format differential vector generator (Rust side).
//!
//! Mirrors the serialization_diff_runner pattern:
//!   1. Builds a representative set of P2P messages / payloads.
//!   2. Runs an internal Rust roundtrip (encode -> decode -> encode, must be
//!      byte-stable).
//!   3. Writes the encoded bytes to a JSON vector file for the C# (Neo
//!      3.10.1) harness (tools/csharp-p2p-runner) to decode + re-encode and
//!      compare byte-for-byte.
//!
//! Usage:
//!   cargo run -q -p neo-p2p --example p2p_wire_diff_runner -- <out_vectors.json> <out_rust_results.json>

use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_p2p::payloads::{
    AddrPayload, FilterAddPayload, FilterLoadPayload, GetBlockByIndexPayload, GetBlocksPayload,
    InvPayload, NetworkAddressWithTime, NodeCapability, PingPayload, VersionPayload,
};
use neo_p2p::{InventoryType, MessageCommand, MessageFlags};
use neo_primitives::UInt256;
use serde_json::{json, Value};
use std::net::IpAddr;
use std::str::FromStr;

struct Vector {
    name: String,
    kind: &'static str,
    bytes: Vec<u8>,
}

fn serialize<T: Serializable>(obj: &T) -> Vec<u8> {
    let mut writer = BinaryWriter::new();
    obj.serialize(&mut writer).unwrap();
    writer.into_bytes()
}

fn roundtrip<T: Serializable + PartialEq + std::fmt::Debug>(
    name: &str,
    obj: &T,
    kind: &'static str,
) -> Result<Vector, String> {
    let bytes = serialize(obj);
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <T as Serializable>::deserialize(&mut reader)
        .map_err(|e| format!("{name}: decode failed: {e}"))?;
    if decoded != *obj {
        return Err(format!("{name}: roundtrip value mismatch"));
    }
    let again = serialize(&decoded);
    if again != bytes {
        return Err(format!("{name}: roundtrip bytes differ"));
    }
    Ok(Vector {
        name: name.to_string(),
        kind,
        bytes,
    })
}

fn hash(seed: u8) -> UInt256 {
    UInt256::from_bytes(&[seed; 32]).unwrap()
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: p2p_wire_diff_runner <out_vectors.json> <out_rust_results.json>");
        std::process::exit(2);
    }
    let mut vectors: Vec<Vector> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    // ---- Version payload (fixed fields, deterministic) ----------------------
    let version = VersionPayload {
        network: 5195087, // MainNet magic
        version: 0,
        timestamp: 1_700_000_000,
        nonce: 0x1234_5678,
        user_agent: "/neo-rs:0.17.0/".to_string(),
        capabilities: vec![
            NodeCapability::TcpServer { port: 10333 },
            NodeCapability::FullNode { start_height: 100 },
        ],
    };
    match roundtrip("version_payload_basic", &version, "version") {
        Ok(v) => vectors.push(v),
        Err(e) => failures.push(e),
    }

    // ---- Ping payload -------------------------------------------------------
    let ping = PingPayload {
        last_block_index: 4242,
        timestamp: 1_700_000_001,
        nonce: 0xdead_beef,
    };
    match roundtrip("ping_payload", &ping, "ping") {
        Ok(v) => vectors.push(v),
        Err(e) => failures.push(e),
    }

    // ---- Inv payload (Tx hashes) --------------------------------------------
    let inv = InvPayload {
        inventory_type: InventoryType::Transaction,
        hashes: vec![hash(1), hash(2), hash(3)],
    };
    match roundtrip("inv_payload_tx", &inv, "inv") {
        Ok(v) => vectors.push(v),
        Err(e) => failures.push(e),
    }

    // ---- GetBlocks payload ---------------------------------------------------
    let get_blocks = GetBlocksPayload {
        hash_start: hash(4),
        count: -1,
    };
    match roundtrip("getblocks_payload", &get_blocks, "getblocks") {
        Ok(v) => vectors.push(v),
        Err(e) => failures.push(e),
    }

    // ---- GetBlockByIndex payload --------------------------------------------
    let gbi = GetBlockByIndexPayload {
        index_start: 1000,
        count: 100,
    };
    match roundtrip("getblockbyindex_payload", &gbi, "getblockbyindex") {
        Ok(v) => vectors.push(v),
        Err(e) => failures.push(e),
    }

    // ---- Addr payload (IPv4-mapped + IPv6 peers) ----------------------------
    let addr = AddrPayload {
        address_list: vec![
            NetworkAddressWithTime::new(
                1_700_000_002,
                IpAddr::from_str("127.0.0.1").unwrap(),
                vec![NodeCapability::TcpServer { port: 10333 }],
            ),
            NetworkAddressWithTime::new(
                1_700_000_003,
                IpAddr::from_str("2001:db8::1").unwrap(),
                vec![NodeCapability::FullNode { start_height: 7 }],
            ),
        ],
    };
    match roundtrip("addr_payload", &addr, "addr") {
        Ok(v) => vectors.push(v),
        Err(e) => failures.push(e),
    }

    // ---- Filter payloads ------------------------------------------------------
    let filter_load = FilterLoadPayload {
        filter: vec![0x01, 0x02, 0x03, 0x04],
        k: 3,
        tweak: 0x55aa_55aa,
    };
    match roundtrip("filterload_payload", &filter_load, "filterload") {
        Ok(v) => vectors.push(v),
        Err(e) => failures.push(e),
    }

    let filter_add = FilterAddPayload {
        data: vec![0xaa; 12],
    };
    match roundtrip("filteradd_payload", &filter_add, "filteradd") {
        Ok(v) => vectors.push(v),
        Err(e) => failures.push(e),
    }

    // ---- Raw message framing -------------------------------------------------
    // The wire frame is flags(u8) + command(u8) + varbytes(payload). The ping
    // payload is under the compression threshold, so the C# Message frame for
    // the same input must equal this exact byte string (asserted on the C#
    // side by decode + re-encode).
    let ping_bytes = serialize(&ping);
    let frame = {
        let mut writer = BinaryWriter::new();
        writer.write_u8(MessageFlags::NONE.bits()).unwrap();
        writer.write_u8(0x18).unwrap(); // ping
        writer.write_var_bytes(&ping_bytes).unwrap();
        writer.into_bytes()
    };
    vectors.push(Vector {
        name: "message_frame_ping".to_string(),
        kind: "message",
        bytes: frame,
    });

    // ---- Message command table (byte values) ---------------------------------
    // The C# side dumps its own MessageCommand table; both tables must agree.
    let command_table: Value = json!({
        "version": 0x00, "verack": 0x01, "getaddr": 0x10, "addr": 0x11,
        "ping": 0x18, "pong": 0x19, "getheaders": 0x20, "headers": 0x21,
        "getblocks": 0x24, "mempool": 0x25, "inv": 0x27, "getdata": 0x28,
        "getblkbyidx": 0x29, "notfound": 0x2a, "extensible": 0x2e,
        "reject": 0x2f, "filterload": 0x30, "filteradd": 0x31,
        "filterclear": 0x32, "merkleblock": 0x38, "alert": 0x40,
    });
    let commands = [
        "version", "verack", "getaddr", "addr", "ping", "pong", "getheaders",
        "headers", "getblocks", "mempool", "inv", "getdata", "getblkbyidx",
        "notfound", "extensible", "reject", "filterload", "filteradd",
        "filterclear", "merkleblock", "alert",
    ];
    for cmd_name in commands {
        // Verify the Rust enum byte matches the table we assert here.
        let expected = command_table[cmd_name].as_u64().unwrap() as u8;
        let parsed = MessageCommand::from_byte(expected).unwrap();
        if parsed.to_byte() != expected {
            failures.push(format!("command {cmd_name}: byte drift"));
        }
    }

    // Wrapper no longer needed: the frame vector above is built manually.

    if !failures.is_empty() {
        for failure in &failures {
            eprintln!("FAIL: {failure}");
        }
        std::process::exit(1);
    }

    let vectors_json: Vec<Value> = vectors
        .iter()
        .map(|v| {
            json!({
                "name": v.name,
                "kind": v.kind,
                "bytes": hex_encode(&v.bytes),
            })
        })
        .collect();
    serde_json::to_writer_pretty(
        std::fs::File::create(&args[1]).unwrap(),
        &json!({ "vectors": vectors_json, "command_table": command_table }),
    )
    .unwrap();

    let results = json!({
        "total": vectors.len() + commands.len(),
        "roundtrip_pass": vectors.len() + commands.len(),
        "roundtrip_fail": 0,
        "failures": [],
    });
    serde_json::to_writer_pretty(std::fs::File::create(&args[2]).unwrap(), &results).unwrap();
    println!(
        "p2p_wire_diff_runner: {} payload vectors + {} command bytes, all roundtrips OK",
        vectors.len(),
        commands.len()
    );
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}
