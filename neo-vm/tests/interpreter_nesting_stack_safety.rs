//! A value cannot nest deeper in the lightweight interpreter than the canonical
//! engine allows, whichever way it tries to get in, and the walks that do run
//! follow the deepest value the bound admits without recursing.
//!
//! That bound is `MaxStackSize`, and a chain of `n` links holds `n` items, so a
//! value built on the stack is now stopped by the item count at the depth the
//! canonical engine stops at. Some wrappings count a link twice while they run
//! (APPEND and SETITEM clone the struct they store, and a map entry is two
//! items), so the depth differs per shape; each one below is the depth both
//! engines accept, and `interpreter_item_limit_differential.rs` pins them to
//! each other at exactly those depths.
//!
//! Every walk the interpreter makes over a value runs on an explicit stack, so
//! the interpreter needs no more native stack at the bound than at depth one.
//! The result it hands back is an ABI `StackValue` whose derived drop and clone
//! glue still costs one native frame set per level; that type lives outside the
//! interpreter, so the threads here use the node's main thread stack, the same
//! 1 MiB the canonical engine's nesting test is anchored to.

use neo_vm::{
    ExecutionResult, MAX_COMPOUND_NESTING_DEPTH, OpCode, StackValue, SyscallProvider, VmState,
    interpret_with_stack_and_syscalls,
};

const NESTING_FAULT: &str = "compound nesting depth exceeds maximum";
const ITEM_COUNT_FAULT: &str = "item count exceeds maximum stack size";
/// The main thread of the node on Windows. Its runtime workers get 2 MiB.
const NODE_MAIN_STACK: usize = 1024 * 1024;
/// Nesting no native stack survives one frame per level of.
const HOSTILE_DEPTH: usize = 100_000;
/// `System.Runtime.Notify`, which declares two arguments.
const NOTIFY_API: u32 = 0x616f_0195;
const TOKEN: u16 = 7;
/// The nesting bound itself: `MaxStackSize` (2048).
const LIMIT: usize = MAX_COMPOUND_NESTING_DEPTH;
/// A depth a few items short of the bound, for the cases that add a child to
/// the value while they run. One native frame per level at this depth fits
/// neither stack size, so a walk that followed the value recursively would take
/// the test process down here.
const DEEP: usize = LIMIT - 8;
/// The deepest mixed chain. A map entry is two items, so a chain that passes
/// through maps costs two items a link and reaches half the bound.
const DEEP_MIXED: usize = LIMIT / 2;

/// Runs `body` on a thread the size of the node's main thread. A native stack
/// overflow takes the whole test process down, so this is where a walk that
/// followed a deep value one frame per level would show up.
fn on_node_stack(body: impl Fn() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(NODE_MAIN_STACK)
        .spawn(body)
        .expect("spawn node-sized thread")
        .join()
        .expect("the case fits the node's main thread stack");
}

/// What the host does with the stack it is lent.
#[derive(Clone, Copy)]
enum Reply {
    /// Hands the values back as they came.
    Unchanged,
    /// Adds an array nested this many levels.
    Nested(usize),
    /// Takes the arguments and leaves an array nested this many levels.
    Swapped(usize),
    /// Fails after adding an array nested this many levels.
    FailsAfterNested(usize),
}

struct Host {
    full_stack: bool,
    reply: Reply,
}

impl Host {
    fn idle() -> Self {
        Self {
            full_stack: false,
            reply: Reply::Unchanged,
        }
    }
}

impl SyscallProvider for Host {
    fn syscall(&mut self, api: u32, _ip: usize, stack: &mut Vec<StackValue>) -> Result<(), String> {
        match self.reply {
            Reply::Unchanged => Ok(()),
            Reply::Nested(levels) => {
                stack.push(nested_arrays(levels));
                Ok(())
            }
            Reply::Swapped(levels) => {
                stack.clear();
                stack.push(nested_arrays(levels));
                Ok(())
            }
            Reply::FailsAfterNested(levels) => {
                stack.push(nested_arrays(levels));
                Err(format!("host call 0x{api:08x} failed"))
            }
        }
    }

    fn needs_full_stack(&self) -> bool {
        self.full_stack
    }
}

/// An ABI array nested `levels` levels, built without recursion.
fn nested_abi(levels: usize) -> StackValue {
    let mut value = StackValue::Array(Vec::new());
    for level in 1..levels {
        value = match level % 3 {
            0 => StackValue::Array(vec![value]),
            1 => StackValue::Struct(vec![value]),
            _ => StackValue::Map(vec![(StackValue::Integer(0), value)]),
        };
    }
    value
}

/// An ABI array nested `levels` levels around an empty array, so every level
/// costs exactly one item. This is the shape that reaches the bound itself.
fn nested_arrays(levels: usize) -> StackValue {
    let mut value = StackValue::Array(Vec::new());
    for _ in 1..levels {
        value = StackValue::Array(vec![value]);
    }
    value
}

fn run(script: &[u8]) -> Result<ExecutionResult, String> {
    interpret_with_stack_and_syscalls(script, Vec::new(), &mut Host::idle())
}

fn assert_halts(outcome: Result<ExecutionResult, String>, label: &str) -> ExecutionResult {
    let result = outcome.unwrap_or_else(|error| panic!("{label}: {error}"));
    assert_eq!(result.state, VmState::Halt, "{label}");
    result
}

/// A value past the bound is refused. A script that builds one link at a time
/// can be stopped by the item count or by the nesting bound, which are the same
/// bound here; either way it must not reach a walk.
fn assert_refused(outcome: Result<ExecutionResult, String>, label: &str) {
    match outcome {
        Err(error) => assert!(
            error == NESTING_FAULT || error == ITEM_COUNT_FAULT,
            "{label}: {error}"
        ),
        Ok(result) => panic!("{label}: reached {:?}", result.state),
    }
}

/// A value handed in from outside is refused by the nesting bound itself,
/// before any of it is imported.
fn assert_nesting_fault(outcome: Result<ExecutionResult, String>, label: &str) {
    match outcome {
        Err(error) => assert_eq!(error, NESTING_FAULT, "{label}"),
        Ok(result) => panic!("{label}: reached {:?}", result.state),
    }
}

fn op(opcode: OpCode) -> Vec<u8> {
    vec![opcode.byte()]
}

fn ops(opcodes: &[OpCode]) -> Vec<u8> {
    opcodes.iter().map(|opcode| opcode.byte()).collect()
}

fn syscall() -> Vec<u8> {
    let mut script = op(OpCode::SYSCALL);
    script.extend(NOTIFY_API.to_le_bytes());
    script
}

fn callt() -> Vec<u8> {
    let mut script = op(OpCode::CALLT);
    script.extend(TOKEN.to_le_bytes());
    script
}

/// Wraps `body` in a TRY whose catch block keeps the exception.
fn caught(body: &[u8]) -> Vec<u8> {
    // TRY and its two offsets, the body, the ENDTRY of the try block.
    let catch_offset = u8::try_from(3 + body.len() + 2).expect("short TRY offset");
    let mut script = vec![OpCode::TRY.byte(), catch_offset, 0];
    script.extend(body);
    script.extend([OpCode::ENDTRY.byte(), 4, OpCode::ENDTRY.byte(), 2]);
    script
}

/// One instruction sequence that puts the value on top of the stack inside a
/// new compound value.
#[derive(Clone, Copy, Debug)]
enum Wrap {
    Pack,
    PackStruct,
    PackMap,
    AppendToArray,
    AppendToStruct,
    SetArrayItem,
    SetMapItem,
}

impl Wrap {
    const ALL: [Self; 7] = [
        Self::Pack,
        Self::PackStruct,
        Self::PackMap,
        Self::AppendToArray,
        Self::AppendToStruct,
        Self::SetArrayItem,
        Self::SetMapItem,
    ];

    fn script(self) -> Vec<u8> {
        use OpCode::*;
        match self {
            Self::Pack => ops(&[PUSH1, PACK]),
            Self::PackStruct => ops(&[PUSH1, PACKSTRUCT]),
            Self::PackMap => ops(&[PUSH0, PUSH1, PACKMAP]),
            Self::AppendToArray => ops(&[NEWARRAY0, DUP, ROT, APPEND]),
            Self::AppendToStruct => ops(&[NEWSTRUCT0, DUP, ROT, APPEND]),
            Self::SetArrayItem => ops(&[PUSH1, NEWARRAY, DUP, ROT, PUSH0, SWAP, SETITEM]),
            Self::SetMapItem => ops(&[NEWMAP, DUP, ROT, PUSH0, SWAP, SETITEM]),
        }
    }

    /// The deepest chain this wrapping builds before the bound stops it.
    ///
    /// PACK and PACKSTRUCT wrap the value in a value that is not itself stored
    /// anywhere, so a link costs one item and the chain reaches the bound. The
    /// other wrappings hold the value while they build the parent (one extra
    /// root, and then one or two children for it), APPEND and SETITEM copy the
    /// struct they store, and a map entry is two items, so each of them stops
    /// that much earlier.
    ///
    /// The canonical engine halts at each of these depths and faults one past
    /// it, which `interpreter_item_limit_differential.rs` checks.
    fn deepest(self) -> usize {
        match self {
            Self::Pack | Self::PackStruct => LIMIT,
            Self::AppendToArray | Self::AppendToStruct => LIMIT - 1,
            Self::SetArrayItem => LIMIT - 3,
            Self::PackMap | Self::SetMapItem => LIMIT / 2,
        }
    }
}

/// `innermost` wrapped until the value on top nests `levels` levels.
fn chain(innermost: OpCode, wrap: Wrap, levels: usize) -> Vec<u8> {
    [op(innermost), wrap.script().repeat(levels - 1)].concat()
}

/// A value nested `levels` levels that mixes every compound kind.
fn mixed_chain(levels: usize) -> Vec<u8> {
    let mut script = op(OpCode::NEWSTRUCT0);
    for level in 1..levels {
        script.extend(Wrap::ALL[level % Wrap::ALL.len()].script());
    }
    script
}

/// The deepest mixed chain both engines reach: measured, and pinned by the
/// differential test one level past as well.
const DEEPEST_MIXED: usize = 1592;

/// A chain that grows at its innermost end, so the value held at the bottom
/// of the stack only gets deeper through alias updates. `rounds` links are
/// added to a root that starts one level deep.
fn chain_grown_from_inside(new_link: &[OpCode], store: &[OpCode], rounds: usize) -> Vec<u8> {
    let round = [ops(new_link), op(OpCode::TUCK), ops(store)].concat();
    [ops(new_link), op(OpCode::DUP), round.repeat(rounds)].concat()
}

/// Each way of linking a value held at the bottom of the stack to a new parent,
/// with the number of rounds that stops at each engine's bound.
const GROWN_FROM_INSIDE: [(&str, &[OpCode], &[OpCode], usize); 3] = [
    (
        "APPEND",
        &[OpCode::NEWARRAY0],
        &[OpCode::APPEND],
        LIMIT - 3,
    ),
    (
        "SETITEM on an array",
        &[OpCode::PUSH1, OpCode::NEWARRAY],
        &[OpCode::PUSH0, OpCode::SWAP, OpCode::SETITEM],
        LIMIT - 6,
    ),
    (
        "SETITEM on a map",
        &[OpCode::NEWMAP],
        &[OpCode::PUSH0, OpCode::SWAP, OpCode::SETITEM],
        LIMIT / 2 - 2,
    ),
];

#[test]
fn every_way_of_nesting_stops_at_the_same_depth_as_the_canonical_engine() {
    on_node_stack(|| {
        for wrap in Wrap::ALL {
            let deepest = wrap.deepest();
            for innermost in [OpCode::NEWARRAY0, OpCode::NEWSTRUCT0, OpCode::NEWMAP] {
                let label = format!("{wrap:?} around {innermost:?}");
                assert_halts(run(&chain(innermost, wrap, deepest)), &label);
                assert_refused(run(&chain(innermost, wrap, deepest + 1)), &label);
            }
        }
        assert_halts(run(&mixed_chain(DEEPEST_MIXED)), "mixed chain");
        assert_refused(run(&mixed_chain(DEEPEST_MIXED + 1)), "mixed chain");
    });
}

#[test]
fn alias_updates_stop_at_the_limit() {
    on_node_stack(|| {
        for (label, new_link, store, deepest) in GROWN_FROM_INSIDE {
            // The root is on the stack only.
            assert_halts(run(&chain_grown_from_inside(new_link, store, deepest)), label);
            assert_refused(
                run(&chain_grown_from_inside(new_link, store, deepest + 1)),
                label,
            );

            // The root also sits in a local, a static field and, one level
            // down, inside another array. Those entries are items of their own,
            // so the chain stops a few links earlier; find where.
            let held_elsewhere = |rounds: usize| {
                let round = [ops(new_link), op(OpCode::TUCK), ops(store)].concat();
                [
                    vec![OpCode::INITSSLOT.byte(), 1, OpCode::INITSLOT.byte(), 1, 0],
                    ops(new_link),
                    ops(&[OpCode::DUP, OpCode::STLOC0, OpCode::DUP, OpCode::STSFLD0]),
                    ops(&[OpCode::DUP, OpCode::PUSH1, OpCode::PACK, OpCode::SWAP]),
                    op(OpCode::DUP),
                    round.repeat(rounds),
                ]
                .concat()
            };
            let extra_roots = (0..=6)
                .map(|shift| deepest - shift)
                .find(|rounds| run(&held_elsewhere(*rounds)).is_ok())
                .unwrap_or_else(|| panic!("{label}: nothing near the bound halts"));
            assert!(
                deepest - extra_roots <= 6,
                "{label}: {} extra items cost {} links",
                deepest - extra_roots,
                deepest - extra_roots
            );
            assert_refused(run(&held_elsewhere(extra_roots + 1)), label);
        }
    });
}

/// `NEWARRAY0`, then `wrap` in a loop that runs `iterations` times.
fn wrapping_loop(innermost: OpCode, wrap: &[u8], iterations: u16) -> Vec<u8> {
    let mut script = vec![OpCode::INITSLOT.byte(), 1, 0, OpCode::PUSHINT16.byte()];
    script.extend(iterations.to_le_bytes());
    script.extend(ops(&[OpCode::STLOC0, innermost]));
    let body = [
        wrap.to_vec(),
        ops(&[OpCode::LDLOC0, OpCode::DEC, OpCode::DUP, OpCode::STLOC0]),
    ]
    .concat();
    let back = i8::try_from(body.len()).expect("short loop body");
    script.extend(&body);
    script.extend([OpCode::JMPIF.byte(), (-back) as u8]);
    script
}

/// The scripts that build thousands of levels for a few GAS: the count stops
/// them now, and the value never gets deep enough to overflow the native stack
/// of a walk that follows it.
#[test]
fn nesting_attacks_fault_instead_of_overflowing_the_native_stack() {
    on_node_stack(|| {
        const ROUNDS: u16 = 5000;
        let rounds = usize::from(ROUNDS);

        let pack = Wrap::Pack.script();
        let pack_map = Wrap::PackMap.script();
        let cases: Vec<(&str, Vec<u8>)> = vec![
            (
                "PUSH1 PACK loop",
                wrapping_loop(OpCode::NEWARRAY0, &pack, ROUNDS),
            ),
            (
                "PACKMAP loop",
                wrapping_loop(OpCode::NEWMAP, &pack_map, ROUNDS),
            ),
            (
                "PUSH1 PACK chain",
                chain(OpCode::NEWARRAY0, Wrap::Pack, rounds),
            ),
            (
                "PUSH1 PACKSTRUCT chain",
                chain(OpCode::NEWSTRUCT0, Wrap::PackStruct, rounds),
            ),
            (
                "PACKMAP nesting",
                chain(OpCode::NEWMAP, Wrap::PackMap, rounds),
            ),
            ("mixed nesting", mixed_chain(rounds)),
        ];
        for (label, script) in cases {
            assert_refused(run(&script), label);
            // Dropping, returning and throwing the value make no difference.
            for tail in [OpCode::DROP, OpCode::RET, OpCode::THROW] {
                assert_refused(run(&[script.clone(), op(tail)].concat()), label);
            }
            // Nor does a TRY around it; only the loops fit a short TRY.
            if script.len() < 100 {
                assert_refused(run(&caught(&script)), label);
            }
        }
        for (label, new_link, store, deepest) in GROWN_FROM_INSIDE {
            assert_refused(
                run(&chain_grown_from_inside(new_link, store, deepest + 1)),
                label,
            );
        }
    });
}

/// The walks the interpreter and its caller make over a value, each at a depth
/// no native stack follows one frame per level.
#[test]
fn deep_values_survive_every_walk() {
    on_node_stack(|| {
        use OpCode::*;
        let slots = vec![INITSSLOT.byte(), 1, INITSLOT.byte(), 1, 0];
        let deepest = mixed_chain(DEEP_MIXED);
        let one_short = mixed_chain(DEEP_MIXED - 1);
        let array_chain = chain(NEWARRAY0, Wrap::Pack, DEEP);
        let struct_chain = chain(NEWSTRUCT0, Wrap::PackStruct, DEEP);

        // Success paths. Each leaves at least one deep value behind for the
        // conversion to the result stack.
        let halting: Vec<(&str, Vec<u8>)> = vec![
            ("returned", deepest.clone()),
            (
                "copied and dropped",
                [deepest.clone(), ops(&[DUP, DUP, DROP])].concat(),
            ),
            (
                "cloned twice by PACK",
                [one_short.clone(), ops(&[DUP, PUSH2, PACK])].concat(),
            ),
            (
                "struct copied by PACKSTRUCT",
                [struct_chain.clone(), ops(&[DUP, PUSH2, PACKSTRUCT])].concat(),
            ),
            (
                "struct copied by APPEND",
                [struct_chain.clone(), ops(&[NEWARRAY0, DUP, ROT, APPEND])].concat(),
            ),
            (
                "struct copied by SETITEM",
                [
                    struct_chain.clone(),
                    ops(&[NEWMAP, DUP, ROT, PUSH0, SWAP, SETITEM]),
                ]
                .concat(),
            ),
            (
                "held by every slot and updated through one alias",
                [
                    slots.clone(),
                    deepest.clone(),
                    ops(&[DUP, STLOC0, DUP, STSFLD0, DUP, DUP, PUSH1, APPEND]),
                    ops(&[LDLOC0, LDSFLD0]),
                ]
                .concat(),
            ),
            (
                "innermost value updated through an alias",
                [
                    array_chain.clone(),
                    ops(&[DUP, PUSH1, PACK, OVER, PUSH1, APPEND]),
                ]
                .concat(),
            ),
            (
                "updated by a callee while a suspended frame holds it",
                [
                    slots.clone(),
                    deepest.clone(),
                    ops(&[DUP, STLOC0]),
                    vec![CALL.byte(), 4],
                    ops(&[LDLOC0, RET]),
                    ops(&[DUP, PUSH1, APPEND, RET]),
                ]
                .concat(),
            ),
            (
                "walked by the collection opcodes",
                [
                    deepest.clone(),
                    ops(&[DUP, VALUES, OVER, UNPACK, DROP, OVER, SIZE, DROP]),
                    ops(&[OVER, OVER, EQUAL, DROP]),
                ]
                .concat(),
            ),
            (
                "thrown and caught",
                [deepest.clone(), caught(&ops(&[DUP, THROW]))].concat(),
            ),
            (
                "thrown out of a callee and caught",
                [
                    slots.clone(),
                    deepest.clone(),
                    ops(&[DUP, STLOC0]),
                    caught(&[CALL.byte(), 6]),
                    op(RET),
                    ops(&[DUP, THROW]),
                ]
                .concat(),
            ),
        ];
        for (label, script) in &halting {
            let result = assert_halts(run(script), label);
            assert!(!result.stack.is_empty(), "{label}");
            // What the caller does with the result.
            let copy = result.clone();
            assert_eq!(copy, result, "{label}");
            assert!(!format!("{copy:?}").is_empty(), "{label}");
        }

        // A chain exactly at the bound halts; one more link is refused where
        // it would be built.
        let at_the_bound = chain(NEWARRAY0, Wrap::Pack, LIMIT);
        assert_halts(run(&at_the_bound), "a chain at the bound");
        assert_refused(
            run(&[at_the_bound, Wrap::Pack.script()].concat()),
            "one link past the bound",
        );

        // The same state torn down by a fault.
        let state = [
            slots.clone(),
            deepest.clone(),
            ops(&[DUP, STLOC0, DUP, STSFLD0, DUP]),
        ]
        .concat();
        for (label, tail) in [
            ("ABORT", op(ABORT)),
            ("uncaught THROW of the value", op(THROW)),
            ("uncaught THROW of a message", ops(&[PUSH1, THROW])),
            ("division by zero", ops(&[PUSH1, PUSH0, DIV])),
            ("stack underflow", ops(&[DROP, DROP, DROP, DROP])),
        ] {
            match run(&[state.clone(), tail].concat()) {
                Ok(result) => {
                    assert_eq!(result.state, VmState::Fault, "{label}");
                    let copy = result.clone();
                    assert_eq!(copy, result, "{label}");
                    assert!(!format!("{copy:?}").is_empty(), "{label}");
                }
                Err(error) => assert!(!error.is_empty(), "{label}"),
            }
        }
        // A fault inside a callee, with the value in the suspended frame.
        let in_callee = [
            slots.clone(),
            deepest.clone(),
            ops(&[DUP, STLOC0]),
            vec![CALL.byte(), 3],
            op(RET),
            ops(&[DUP, PUSH1, PUSH0, DIV]),
        ]
        .concat();
        assert!(run(&in_callee).is_err(), "fault in a callee");
    });
}

/// Runs `script` against a host that answers every call with `reply`.
fn run_with_host(
    script: &[u8],
    initial_stack: Vec<StackValue>,
    full_stack: bool,
    reply: Reply,
) -> Result<ExecutionResult, String> {
    interpret_with_stack_and_syscalls(script, initial_stack, &mut Host { full_stack, reply })
}

/// Two values of this depth plus the copy a script makes fit inside the bound.
const PAIR: usize = LIMIT / 2 - 4;

/// Three values of this depth fit inside the bound.
const TRIO: usize = (LIMIT - 8) / 3;

#[test]
fn deep_values_cross_the_host_boundary_both_ways() {
    on_node_stack(|| {
        // The deepest value that can come in through the initial stack: the
        // bound itself, which costs the whole item budget on its own.
        assert_halts(
            run_with_host(
                &op(OpCode::DROP),
                vec![nested_arrays(LIMIT)],
                false,
                Reply::Unchanged,
            ),
            "initial stack at the bound",
        );

        for (path, call, full_stack) in [
            ("syscall", syscall(), false),
            ("full-stack syscall", syscall(), true),
            ("CALLT", callt(), false),
        ] {
            let script = [ops(&[OpCode::PUSH0, OpCode::PUSH0]), call.clone()].concat();

            // The host takes the arguments and leaves a value at the bound.
            let result = assert_halts(
                run_with_host(&script, Vec::new(), full_stack, Reply::Swapped(LIMIT)),
                path,
            );
            assert_eq!(result.stack, vec![nested_arrays(LIMIT)], "{path}");

            // A deep value handed back next to the arguments still on the
            // stack, so the interpreter keeps both.
            let result = assert_halts(
                run_with_host(&script, Vec::new(), full_stack, Reply::Nested(PAIR)),
                path,
            );
            assert_eq!(result.stack.len(), 3, "{path}");
            assert_eq!(result.stack.last(), Some(&nested_arrays(PAIR)), "{path}");

            // A deep value out as an argument and back, with the copy the
            // script made of it: three deep values that all stay inside the
            // bound, two of them sharing the value the host was lent.
            let script = [
                chain(OpCode::NEWARRAY0, Wrap::Pack, TRIO),
                op(OpCode::DUP),
                call.clone(),
            ]
            .concat();
            let result = assert_halts(
                run_with_host(
                    &script,
                    vec![nested_arrays(TRIO)],
                    full_stack,
                    Reply::Unchanged,
                ),
                path,
            );
            assert_eq!(result.stack.len(), 3, "{path}");

            // The host fails with a deep value it already took: uncaught it
            // is a fault, and a TRY around it is a halt.
            assert!(
                run_with_host(
                    &call,
                    vec![nested_arrays(TRIO)],
                    full_stack,
                    Reply::FailsAfterNested(TRIO),
                )
                .is_err(),
                "{path}"
            );
            let result = assert_halts(
                run_with_host(
                    &caught(&call),
                    vec![nested_arrays(TRIO)],
                    full_stack,
                    Reply::FailsAfterNested(TRIO),
                ),
                path,
            );
            assert_eq!(result.state, VmState::Halt, "{path}");
        }
    });
}

#[test]
fn values_past_the_limit_are_refused_at_every_entry() {
    on_node_stack(|| {
        for levels in [LIMIT + 1, 2 * LIMIT, HOSTILE_DEPTH] {
            // The initial stack.
            let label = format!("initial stack, {levels} levels");
            assert_nesting_fault(
                run_with_host(
                    &op(OpCode::DROP),
                    vec![StackValue::Integer(1), nested_abi(levels)],
                    false,
                    Reply::Unchanged,
                ),
                &label,
            );

            for (path, call, full_stack) in [
                ("syscall", syscall(), false),
                ("full-stack syscall", syscall(), true),
                ("CALLT", callt(), false),
            ] {
                let label = format!("{path}, {levels} levels");
                let arguments = || vec![StackValue::Integer(1), StackValue::Integer(2)];
                // What a successful host call hands back.
                assert_nesting_fault(
                    run_with_host(&call, arguments(), full_stack, Reply::Nested(levels)),
                    &label,
                );
                // A script cannot catch the refusal and carry on.
                assert_nesting_fault(
                    run_with_host(
                        &caught(&call),
                        arguments(),
                        full_stack,
                        Reply::Nested(levels),
                    ),
                    &label,
                );
                // A failed call leaves the value with the interpreter to free.
                let outcome = run_with_host(
                    &call,
                    arguments(),
                    full_stack,
                    Reply::FailsAfterNested(levels),
                );
                assert!(outcome.is_err(), "{label}");
                let result = assert_halts(
                    run_with_host(
                        &caught(&call),
                        arguments(),
                        full_stack,
                        Reply::FailsAfterNested(levels),
                    ),
                    &label,
                );
                assert!(
                    result.stack.iter().all(|value| matches!(
                        value,
                        StackValue::Integer(_) | StackValue::ByteString(_)
                    )),
                    "{label}: {:?}",
                    result.stack.len()
                );
            }
        }
    });
}
