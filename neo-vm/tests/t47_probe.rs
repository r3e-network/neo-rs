//! RECONSTRUCTION, not the original. The `t47_probe.rs` that task 47 used was
//! never committed and no longer exists; this file was written afterwards,
//! during the task 47 wrap-up, to measure the canonical engine again for
//! task 52. Nothing in it produced the numbers quoted in the task 47 commit
//! messages. It is left uncommitted on purpose.
//!
//! Every test is ignored: they print measurements instead of asserting, one
//! does not return on its own and one takes the process down with a native
//! stack overflow.
//!
//! ```text
//! cargo test -p neo-vm --test t47_probe -- --ignored --nocapture canonical_item_count
//! cargo test -p neo-vm --test t47_probe -- --ignored --nocapture canonical_self_reference
//! T47_MODE=append T47_DEPTH=20000 T47_STACK=1048576 \
//!     cargo test -p neo-vm --test t47_probe -- --ignored --nocapture canonical_deep_chain
//! ```

use neo_vm::{
    ExecutionEngine, ExecutionEngineLimits, OpCode, Script, StackItem, VmState,
    interpret_with_stack_and_syscalls,
};
use std::collections::HashSet;
use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

fn op(opcode: OpCode) -> Vec<u8> {
    vec![opcode.byte()]
}

fn ops(opcodes: &[OpCode]) -> Vec<u8> {
    opcodes.iter().map(|opcode| opcode.byte()).collect()
}

fn int(value: usize) -> Vec<u8> {
    if value <= 16 {
        vec![OpCode::PUSH0.byte() + value as u8]
    } else {
        let mut script = vec![OpCode::PUSHINT32.byte()];
        script.extend(i32::try_from(value).expect("small operand").to_le_bytes());
        script
    }
}

fn pushes(count: usize) -> Vec<u8> {
    op(OpCode::PUSH1).repeat(count)
}

fn new_array(len: usize) -> Vec<u8> {
    [int(len), op(OpCode::NEWARRAY)].concat()
}

fn engine_for(script: &[u8]) -> ExecutionEngine {
    let mut engine = ExecutionEngine::new(None);
    engine.set_limits(ExecutionEngineLimits::DEFAULT);
    engine
        .load_script(Script::new_relaxed(script.to_vec()), -1, 0)
        .expect("load probe script");
    engine
}

/// What a counter that follows every reference reports for `roots`: one per
/// stack entry, plus one per child of each distinct Array, Struct and Map
/// reachable from them, a Map entry being a key and a value.
fn reachable_references(roots: &[StackItem]) -> usize {
    let mut count = roots.len();
    let mut seen = HashSet::new();
    let mut pending = roots.to_vec();
    while let Some(item) = pending.pop() {
        let (identity, children): ((u8, usize), Vec<StackItem>) = match &item {
            StackItem::Array(array) => ((0, array.id()), array.items()),
            StackItem::Struct(structure) => ((1, structure.id()), structure.items()),
            StackItem::Map(map) => (
                (2, map.id()),
                map.iter().flat_map(|(key, value)| [key, value]).collect(),
            ),
            _ => continue,
        };
        if seen.insert(identity) {
            count += children.len();
            pending.extend(children);
        }
    }
    count
}

/// One straight-line script stepped on the canonical engine. The counts are
/// taken after every instruction, before the implicit RET.
struct Measured {
    outcome: String,
    /// The reference counter and the reachable references after the last
    /// instruction that ran.
    last: (usize, usize),
    /// The same pair at the instruction where the counter lagged the most.
    widest: (usize, usize),
}

fn measure(script: &[u8]) -> Measured {
    let mut engine = engine_for(script);
    let mut last: (usize, usize) = (0, 0);
    let mut widest: (usize, usize) = (0, 0);
    let outcome = loop {
        let Some(context) = engine.current_context() else {
            break "ran off the invocation stack".to_string();
        };
        if context.instruction_pointer() >= script.len() {
            break "Halt".to_string();
        }
        if let Err(error) = engine.execute_next() {
            break format!("Fault ({error})");
        }
        let Some(context) = engine.current_context() else {
            break "ran off the invocation stack".to_string();
        };
        let roots: Vec<StackItem> = context.evaluation_stack().iter().cloned().collect();
        last = (
            engine.reference_counter().count(),
            reachable_references(&roots),
        );
        if last.1.saturating_sub(last.0) > widest.1.saturating_sub(widest.0) {
            widest = last;
        }
    };
    Measured {
        outcome,
        last,
        widest,
    }
}

fn lightweight(script: &[u8]) -> String {
    struct NoHost;
    impl neo_vm::SyscallProvider for NoHost {
        fn syscall(
            &mut self,
            api: u32,
            _ip: usize,
            _stack: &mut Vec<neo_vm::StackValue>,
        ) -> Result<(), String> {
            Err(format!("unexpected syscall 0x{api:08x}"))
        }
    }
    match interpret_with_stack_and_syscalls(script, Vec::new(), &mut NoHost) {
        Ok(result) if result.state == VmState::Halt => "Halt".to_string(),
        Ok(result) => format!("Fault ({})", result.fault_message.unwrap_or_default()),
        Err(error) => format!("Fault ({error})"),
    }
}

/// The canonical reference counter against the reachable references, for
/// minimal shapes and for the scripts of
/// `shapes_the_canonical_counter_undercounts_never_fault_on_the_item_count`
/// that have no slots (the stack is then the only root).
#[test]
#[ignore = "probe: prints measurements"]
fn canonical_item_count() {
    use OpCode::*;
    let append_array = [ops(&[NEWARRAY0, DUP]), new_array(100), op(APPEND)].concat();
    let cases: Vec<(&str, Vec<u8>)> = vec![
        // Controls: no value is stored into an existing compound.
        (
            "control: PACK of a 100-array",
            [new_array(100), int(1), op(PACK)].concat(),
        ),
        (
            "control: 100 primitives appended",
            [op(NEWARRAY0), ops(&[DUP, PUSH1, APPEND]).repeat(100)].concat(),
        ),
        // Minimal shapes.
        ("100-array appended to an array", append_array.clone()),
        (
            "500-array stored with SETITEM into an array",
            [
                new_array(1),
                ops(&[DUP, PUSH0]),
                new_array(500),
                op(SETITEM),
            ]
            .concat(),
        ),
        (
            "500-array stored with SETITEM into a map",
            [ops(&[NEWMAP, DUP, PUSH0]), new_array(500), op(SETITEM)].concat(),
        ),
        (
            "buffer appended to an array",
            ops(&[NEWARRAY0, DUP, PUSH4, NEWBUFFER, APPEND]),
        ),
        (
            "primitive appended to an array held only by its parent",
            [
                ops(&[NEWARRAY0, PUSH1, PACK]),
                ops(&[DUP, PUSH0, PICKITEM, PUSH1, APPEND]).repeat(100),
            ]
            .concat(),
        ),
        (
            "chain of 62 grown by APPEND",
            [
                ops(&[NEWARRAY0, DUP]),
                ops(&[NEWARRAY0, TUCK, APPEND]).repeat(62),
            ]
            .concat(),
        ),
        // The other direction: the counter stays above what is reachable.
        (
            "300-array removed from the array it was packed into",
            [new_array(300), int(1), op(PACK), ops(&[DUP, PUSH0, REMOVE])].concat(),
        ),
        // The scripts of the differential test.
        (
            "diff: array appended to an array",
            [append_array.repeat(30), pushes(1900)].concat(),
        ),
        (
            "diff: array stored with SETITEM",
            [
                [
                    new_array(1),
                    ops(&[DUP, PUSH0]),
                    new_array(500),
                    op(SETITEM),
                ]
                .concat()
                .repeat(10),
                pushes(2000),
            ]
            .concat(),
        ),
        (
            "diff: array stored in a map",
            [
                [ops(&[NEWMAP, DUP, PUSH0]), new_array(500), op(SETITEM)]
                    .concat()
                    .repeat(10),
                pushes(2000),
            ]
            .concat(),
        ),
        (
            "diff: buffer appended to an array",
            [
                ops(&[NEWARRAY0, DUP, PUSH4, NEWBUFFER, APPEND]).repeat(600),
                pushes(1400),
            ]
            .concat(),
        ),
        (
            "diff: parent released with its appended child",
            ops(&[NEWARRAY0, NEWARRAY0, OVER, OVER, APPEND, NIP]).repeat(1500),
        ),
        (
            "diff: one struct packed many times",
            [
                [int(100), op(NEWSTRUCT)].concat(),
                op(DUP).repeat(20),
                int(21),
                op(PACK),
                pushes(1900),
            ]
            .concat(),
        ),
        (
            "diff: struct appended to an array",
            [
                [
                    ops(&[NEWARRAY0, DUP]),
                    [int(100), op(NEWSTRUCT)].concat(),
                    op(APPEND),
                ]
                .concat()
                .repeat(30),
                pushes(1900),
            ]
            .concat(),
        ),
        (
            "diff: children removed from a packed array",
            [
                [new_array(300), int(1), op(PACK), ops(&[DUP, PUSH0, REMOVE])]
                    .concat()
                    .repeat(6),
                pushes(2000),
            ]
            .concat(),
        ),
    ];

    for (label, script) in cases {
        let measured = measure(&script);
        println!(
            "T47 count | {label} | canonical {} | at end: counter {} reachable {} | widest gap: counter {} reachable {} | lightweight {}",
            measured.outcome,
            measured.last.0,
            measured.last.1,
            measured.widest.0,
            measured.widest.1,
            lightweight(&script),
        );
    }
}

fn environment(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn report(line: &str) {
    println!("{line}");
    std::io::stdout().flush().expect("flush probe output");
}

/// Builds a chain `T47_DEPTH` levels deep on a thread of `T47_STACK` bytes
/// and lets the canonical engine tear it down. `T47_MODE`:
///
/// - `append`: the chain grows at its innermost end and stays on the stack,
///   so it is freed when the engine is dropped;
/// - `append_drop`: the same chain, dropped by the script itself;
/// - `pack`: `PUSH1 PACK` around the value, which the item limit stops at
///   2048 levels.
///
/// The stage printed last tells how far the run got before the process died.
#[test]
#[ignore = "probe: overflows the native stack past some depth"]
fn canonical_deep_chain() {
    use OpCode::*;
    let depth = environment("T47_DEPTH", 1000);
    let stack = environment("T47_STACK", 1024 * 1024);
    let mode = std::env::var("T47_MODE").unwrap_or_else(|_| "append".to_string());

    let grown = [
        ops(&[NEWARRAY0, DUP]),
        ops(&[NEWARRAY0, TUCK, APPEND]).repeat(depth.saturating_sub(1)),
    ]
    .concat();
    let script = match mode.as_str() {
        "append" => grown,
        "append_drop" => [grown, ops(&[DROP, DROP])].concat(),
        "pack" => [
            op(NEWARRAY0),
            ops(&[PUSH1, PACK]).repeat(depth.saturating_sub(1)),
        ]
        .concat(),
        other => panic!("unknown T47_MODE {other}"),
    };

    let tag = format!("T47 chain mode={mode} depth={depth} stack={stack}");
    std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            let mut engine = engine_for(&script);
            report(&format!("{tag} stage=loaded"));
            let state = engine.execute();
            report(&format!(
                "{tag} stage=executed state={state:?} counter={}",
                engine.reference_counter().count()
            ));
            drop(engine);
            report(&format!("{tag} stage=dropped"));
        })
        .expect("spawn probe thread")
        .join()
        .expect("probe thread completes");
}

/// A collection stored into itself. Each script runs on its own thread; one
/// that has not answered after ten seconds is reported and left behind.
#[test]
#[ignore = "probe: the canonical engine does not return"]
fn canonical_self_reference() {
    use OpCode::*;
    let cases: Vec<(&str, Vec<u8>)> = vec![
        (
            "control: A.append(1)",
            ops(&[NEWARRAY0, DUP, PUSH1, APPEND]),
        ),
        ("A.append(A)", ops(&[NEWARRAY0, DUP, DUP, APPEND])),
        (
            "A[0] = A",
            [new_array(1), ops(&[DUP, PUSH0, OVER, SETITEM])].concat(),
        ),
        ("M[0] = M", ops(&[NEWMAP, DUP, PUSH0, OVER, SETITEM])),
    ];
    let mut stuck = false;
    for (label, script) in cases {
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let mut engine = engine_for(&script);
            let state = engine.execute();
            let counter = engine.reference_counter().count();
            // Tearing a cycle down is a separate question.
            std::mem::forget(engine);
            let _ = sender.send((state, counter));
        });
        match receiver.recv_timeout(Duration::from_secs(10)) {
            Ok((state, counter)) => {
                report(&format!("T47 self | {label} | {state:?} counter {counter}"));
            }
            Err(_) => {
                stuck = true;
                report(&format!("T47 self | {label} | no result within 10 s"));
            }
        }
    }
    if stuck {
        // The stuck threads would keep the harness from exiting.
        std::process::exit(0);
    }
}
