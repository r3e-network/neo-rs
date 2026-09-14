//! Cross-implementation differential execution runner (Rust side).
//!
//! Reads a JSON vector file, executes each script on a bare `ExecutionEngine`,
//! and writes a JSON result file using a transport-neutral schema that the C#
//! harness (`tools/csharp-vm-runner`) also emits. See `scripts/vm-diff.py`.
//!
//! Usage:
//!   cargo run -p neo-vm --example vm_diff_runner -- <in.json> <out.json>
//!
//! Deliberately uses a bare engine: no host, no syscalls, no storage. Vectors
//! that reference syscalls are expected to FAULT on both sides identically.
//!
//! The result stack is rendered with the crate's own Neo JSON-RPC stack
//! envelope (`stack_item_rpc_json`) rather than a bespoke serializer, so the
//! comparison uses one canonical form that both implementations already agree
//! on (same type names, same byte order, same base64 convention).

use std::fs;

use neo_vm::{ExecutionEngine, Script, VmState, stack_items_rpc_json_per_item};
use serde::{Deserialize, Serialize};
use serde_json::Value;

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
    #[serde(default)]
    rvcount: Option<i32>,
}

#[derive(Debug, Serialize)]
struct ResultFile {
    impl_name: String,
    results: Vec<VectorResult>,
}

#[derive(Debug, Serialize)]
struct VectorResult {
    name: String,
    /// "HALT" | "FAULT" | "NONE" | "BREAK"
    state: String,
    /// Result stack, bottom-first, in the shared Neo RPC envelope.
    stack: Vec<Value>,
    /// Fault classification (exception family), or null when not faulted.
    fault: Option<String>,
    /// Set when this harness itself could not run the vector.
    #[serde(skip_serializing_if = "Option::is_none")]
    harness_error: Option<String>,
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: vm_diff_runner <in.json> <out.json>");
        std::process::exit(2);
    }

    let raw = fs::read_to_string(&args[1]).expect("read vector file");
    let file: VectorFile = serde_json::from_str(&raw).expect("parse vector file");

    let mut results = Vec::new();
    for vector in &file.vectors {
        results.push(run_one(vector));
    }

    let out = ResultFile {
        impl_name: "rust-neo-vm".to_string(),
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

    let mut engine = ExecutionEngine::new(None);
    let mut fault = None;

    let script = Script::new_relaxed(code);
    let rvcount = vector.rvcount.unwrap_or(-1);
    if let Err(err) = engine.load_script(script, rvcount, 0) {
        fault = Some(classify(&err.to_string()));
    } else {
        engine.execute();
        // `execute()` runs to HALT/FAULT; a fault is surfaced through the
        // uncaught exception rather than a Result.
        if let Some(exception) = engine.uncaught_exception() {
            fault = Some(classify(&format!("{exception:?}")));
        }
    }

    let state = engine.state();
    let state_name = match state.final_name() {
        Some(name) => name.to_string(),
        None => match state {
            VmState::None => "NONE".to_string(),
            VmState::Break => "BREAK".to_string(),
            VmState::Halt => "HALT".to_string(),
            VmState::Fault => "FAULT".to_string(),
        },
    };

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
        fault,
        harness_error,
    }
}

fn decode_hex(s: &str) -> Result<Vec<u8>, String> {
    let s = s.trim();
    if s.len() % 2 != 0 {
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

/// Coarse fault classification.
///
/// Deliberately generic: the two implementations do not share exception type
/// names, so we compare *families*. Written to avoid over-fitting to either
/// side's wording.
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
