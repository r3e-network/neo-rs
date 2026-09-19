//! Cross-implementation syscall differential execution runner (Rust side).
//!
//! Runs syscall scripts on a hosted `ApplicationEngine` (the layer that actually
//! registers and executes system calls & native contracts), producing the SAME
//! result-envelope schema as `neo-vm/examples/vm_diff_runner.rs` so
//! `scripts/vm-diff.py` can compare against the C# host runner
//! (`tools/csharp-app-runner`).
//!
//! Usage:
//!   cargo run -p neo-core --example vm_syscall_runner -- <in.json> <out.json>
//!
//! Environment (must match the C# runner exactly):
//!   - ProtocolSettings::default()      (network=0, address_version=53)
//!   - TriggerType::Application
//!   - a minimal Transaction container (crypto FALSE/FAULT paths only)
//!   - a persisting block, header timestamp = PINNED_BLOCK_TIMESTAMP
//! The native-contract storage is left empty; neo-rs ApplicationEngine falls
//! back to defaults for policy/latency reads, so no genesis seeding is needed
//! here (the C# side must seed Ledger+Policy keys; see its runner).

use std::fs;
use std::sync::Arc;

use neo_core::ledger::Block;
use neo_core::ledger::block_header::BlockHeader;
use neo_core::network::p2p::payloads::Transaction;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::CallFlags;
use neo_core::smart_contract::TriggerType;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_vm::{VmState, stack_items_rpc_json_per_item};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Fixed persisting-block header timestamp shared with the C# runner.
const PINNED_BLOCK_TIMESTAMP: u64 = 1_700_000_000;

/// Independent per-item render budget for the RPC envelope (bytes).
const RPC_STACK_BUDGET: usize = 2 * 1024 * 1024;

#[derive(Debug, Deserialize)]
struct VectorFile {
    vectors: Vec<Vector>,
}

#[derive(Debug, Deserialize)]
struct Vector {
    name: String,
    script: String,
}

#[derive(Debug, Serialize)]
struct ResultFile {
    impl_name: String,
    results: Vec<VectorResult>,
}

#[derive(Debug, Serialize)]
struct VectorResult {
    name: String,
    state: String,
    stack: Vec<Value>,
    fault: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    harness_error: Option<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: vm_syscall_runner <in.json> <out.json>");
        std::process::exit(2);
    }
    let raw = fs::read_to_string(&args[1]).expect("read vector file");
    let file: VectorFile = serde_json::from_str(&raw).expect("parse vector file");

    let mut results = Vec::new();
    for vector in &file.vectors {
        results.push(run_one(vector));
    }
    let out = ResultFile {
        impl_name: "rust-neo-rs-application-engine".to_string(),
        results,
    };
    fs::write(&args[2], serde_json::to_string_pretty(&out).unwrap()).expect("write result file");
}

fn run_one(vector: &Vector) -> VectorResult {
    let code = match decode_hex(&vector.script) {
        Ok(code) => code,
        Err(err) => {
            return VectorResult {
                name: vector.name.clone(),
                state: "HARNESS_ERROR".into(),
                stack: Vec::new(),
                fault: None,
                harness_error: Some(err),
            };
        }
    };

    let mut engine = match build_engine() {
        Ok(e) => e,
        Err(err) => {
            return VectorResult {
                name: vector.name.clone(),
                state: "HARNESS_ERROR".into(),
                stack: Vec::new(),
                fault: None,
                harness_error: Some(format!("engine build failed: {err}")),
            };
        }
    };

    let mut fault: Option<String> = None;
    if let Err(err) = engine.load_script(code, CallFlags::ALL, None) {
        fault = Some(classify(&err.to_string()));
    } else {
        match engine.execute() {
            Ok(_) => {}
            Err(_) => {
                if let Some(msg) = engine.fault_exception_string() {
                    fault = Some(classify(msg));
                } else {
                    fault = Some(classify(&stringify("execute failed")));
                }
            }
        }
    }

    let state = engine.state();
    let state_name = state.final_name().unwrap_or("NONE").to_string();

    let items: Vec<neo_vm::StackItem> = engine.result_stack().iter().cloned().collect();
    let (stack, harness_error) = match stack_items_rpc_json_per_item(&items, RPC_STACK_BUDGET) {
        Ok(values) => (values, None),
        Err(err) => (
            Vec::new(),
            Some(format!("stack render failed: {err:?}")),
        ),
    };

    VectorResult {
        name: vector.name.clone(),
        state: state_name,
        stack,
        fault: fault.or(if state == VmState::Fault {
            Some("other".into())
        } else {
            None
        }),
        harness_error,
    }
}

fn stringify(s: &str) -> String {
    s.to_string()
}

/// Builds the pinned hosted engine used by every vector.
fn build_engine() -> Result<ApplicationEngine, String> {
    let snapshot = Arc::new(DataCache::new(false));

    let header = BlockHeader::new(
        1,                                   // index
        Default::default(),                  // prev_hash
        Default::default(),                  // merkle_root
        PINNED_BLOCK_TIMESTAMP,              // timestamp
        0,
        0,
        0,
        neo_core::UInt160::zero(),
        Vec::new(),
    );
    let block = Block::new(header, Vec::new());

    let tx = Transaction::new();
    let container: Arc<dyn neo_core::Verifiable> = Arc::new(tx);

    ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        Some(block),
        ProtocolSettings::default(),
        20_000_000_000,
        None,
    )
    .map_err(|e| e.to_string())
}

fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) {
        return Err(format!("odd-length hex script ({} chars)", s.len()));
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char)
            .to_digit(16)
            .ok_or_else(|| format!("bad hex at {i}"))? as u8;
        let lo = (bytes[i + 1] as char)
            .to_digit(16)
            .ok_or_else(|| format!("bad hex at {}", i + 1))? as u8;
        out.push((hi << 4) | lo);
        i += 2;
    }
    Ok(out)
}

/// Coarse fault classification, kept in lockstep with the C# host runner.
fn classify(message: &str) -> String {
    let m = message.to_ascii_lowercase();
    if m.contains("overflow") {
        "overflow".into()
    } else if m.contains("underflow")
        || m.contains("stack is empty")
        || m.contains("empty stack")
        || m.contains("insufficient")
        || m.contains("pop")
    {
        "underflow".into()
    } else if m.contains("divide") || m.contains("division") || m.contains("modulo by zero") {
        "divzero".into()
    } else if m.contains("index") || m.contains("out of bounds") || m.contains("range") {
        "range".into()
    } else if m.contains("public key") || m.contains("ecpoint") || m.contains("pubkey") {
        "invalid".into()
    } else if m.contains("type")
        || m.contains("cast")
        || m.contains("expected")
        || m.contains("not a")
    {
        "type".into()
    } else if m.contains("unexpected") || m.contains("invalid") || m.contains("bad") {
        "invalid".into()
    } else {
        "other".into()
    }
}