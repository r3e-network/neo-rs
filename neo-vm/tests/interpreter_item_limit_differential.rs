//! The lightweight interpreter reaches the same state as the canonical engine
//! on the item count and on compound nesting: it faults exactly where the
//! canonical engine faults, and halts everywhere the canonical engine halts.

use neo_vm::{
    ExecutionEngine, ExecutionEngineLimits, InterpreterSemantics, MAX_COMPOUND_NESTING_DEPTH,
    OpCode, Script, StackValue, SyscallProvider, VmState, interpret_with_stack_and_syscalls,
};

const ITEM_COUNT_FAULT: &str = "item count exceeds maximum stack size";
/// `StackItemType` codes that CONVERT takes.
const ARRAY_TYPE: u8 = 0x40;
const STRUCT_TYPE: u8 = 0x41;

struct PureHost;

impl SyscallProvider for PureHost {
    fn semantics(&self) -> InterpreterSemantics {
        InterpreterSemantics {
            zero_shift_converts_to_integer: true,
            pre_gorgon_collection_semantics: false,
        }
    }

    fn syscall(
        &mut self,
        api: u32,
        _ip: usize,
        _stack: &mut Vec<StackValue>,
    ) -> Result<(), String> {
        Err(format!("unexpected syscall 0x{api:08x}"))
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Fast {
    Halt,
    Fault(String),
}

impl Fast {
    fn state(&self) -> VmState {
        match self {
            Self::Halt => VmState::Halt,
            Self::Fault(_) => VmState::Fault,
        }
    }
}

fn run_fast(script: &[u8]) -> Fast {
    run_fast_with(script, Vec::new(), &mut PureHost)
}

fn run_fast_with(script: &[u8], stack: Vec<StackValue>, host: &mut impl SyscallProvider) -> Fast {
    match interpret_with_stack_and_syscalls(script, stack, host) {
        Ok(result) if result.state == VmState::Halt => Fast::Halt,
        Ok(result) => Fast::Fault(result.fault_message.unwrap_or_default()),
        Err(error) => Fast::Fault(error),
    }
}

fn run_canonical(script: &[u8]) -> VmState {
    let mut engine = ExecutionEngine::new(None);
    engine.set_limits(ExecutionEngineLimits::DEFAULT);
    engine
        .load_script(Script::new_relaxed(script.to_vec()), -1, 0)
        .expect("load differential script");
    engine.execute()
}

/// Both engines reach the same state.
fn assert_same_state(script: &[u8], label: &str) -> VmState {
    let fast = run_fast(script);
    let canonical = run_canonical(script);
    assert_eq!(fast.state(), canonical, "{label}: lightweight {fast:?}");
    canonical
}

/// Both engines halt on `build(n)` up to `last_halt` and fault past it.
fn assert_boundary(label: &str, last_halt: usize, build: impl Fn(usize) -> Vec<u8>) {
    for n in last_halt - 2..=last_halt + 2 {
        let expected = if n <= last_halt {
            VmState::Halt
        } else {
            VmState::Fault
        };
        let label = format!("{label}, n = {n}");
        assert_eq!(assert_same_state(&build(n), &label), expected, "{label}");
    }
}

fn op(opcode: OpCode) -> Vec<u8> {
    vec![opcode.byte()]
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

fn init_slot(locals: u8, arguments: u8) -> Vec<u8> {
    vec![OpCode::INITSLOT.byte(), locals, arguments]
}

fn init_static_slot(count: u8) -> Vec<u8> {
    vec![OpCode::INITSSLOT.byte(), count]
}

/// Runs `body` where neither engine's teardown of a long chain can exhaust
/// the native stack.
fn on_large_stack(body: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .stack_size(256 * 1024 * 1024)
        .spawn(body)
        .expect("spawn large-stack thread")
        .join()
        .expect("differential case completes");
}

#[test]
fn item_count_boundary_matches_canonical_for_plain_stack_entries() {
    assert_boundary("plain pushes", 2048, pushes);
}

#[test]
fn item_count_boundary_matches_canonical_for_wide_shallow_collections() {
    assert_boundary("NEWARRAY", 2047, new_array);
    assert_boundary("NEWSTRUCT", 2047, |n| {
        [int(n), op(OpCode::NEWSTRUCT)].concat()
    });
    // One item of headroom is left after the array and is then used up.
    assert_boundary("NEWARRAY and pushes", 48, |n| {
        [new_array(1999), pushes(n)].concat()
    });
}

#[test]
fn item_count_boundary_matches_canonical_for_one_compound_referenced_many_times() {
    // The same array on the stack over and over: its children count once.
    assert_boundary("DUP of one array", 1047, |n| {
        [new_array(1000), op(OpCode::DUP).repeat(n)].concat()
    });
    // The same array as every child of another array.
    assert_boundary("PACK of one array", 1047, |n| {
        [
            new_array(1000),
            op(OpCode::DUP).repeat(n - 1),
            int(n),
            op(OpCode::PACK),
        ]
        .concat()
    });
    // The same array on the stack, in a local and in a static field.
    assert_boundary("one array in stack and slots", 1045, |n| {
        [
            init_static_slot(1),
            init_slot(1, 0),
            new_array(1000),
            op(OpCode::DUP),
            op(OpCode::STLOC0),
            op(OpCode::DUP),
            op(OpCode::STSFLD0),
            pushes(n),
        ]
        .concat()
    });
    // The same array as two values of one map.
    assert_boundary("one array twice in a map", 1143, |n| {
        [
            new_array(900),
            int(0),
            op(OpCode::OVER),
            int(1),
            int(2),
            op(OpCode::PACKMAP),
            pushes(n),
        ]
        .concat()
    });
}

#[test]
fn item_count_boundary_matches_canonical_for_maps() {
    let map = |entries: usize| {
        let mut script = Vec::new();
        for key in 0..entries {
            script.extend(int(1));
            script.extend(int(key));
        }
        script.extend(int(entries));
        script.extend(op(OpCode::PACKMAP));
        script
    };

    // A map entry is two items.
    assert_boundary("PACKMAP and pushes", 47, |n| {
        [map(1000), pushes(n)].concat()
    });
    assert_eq!(assert_same_state(&map(1023), "PACKMAP 1023"), VmState::Halt);
    assert_eq!(
        assert_same_state(&map(1024), "PACKMAP 1024"),
        VmState::Fault
    );
}

#[test]
fn item_count_boundary_matches_canonical_for_nested_collections() {
    let arrays = |count: usize| new_array(100).repeat(count);

    assert_boundary("arrays in an array", 27, |n| {
        [arrays(20), int(20), op(OpCode::PACK), pushes(n)].concat()
    });
    assert_boundary("arrays in a struct", 27, |n| {
        [arrays(20), int(20), op(OpCode::PACKSTRUCT), pushes(n)].concat()
    });
    assert_boundary("arrays in a map", 7, |n| {
        let mut script = Vec::new();
        for key in 0..20 {
            script.extend(new_array(100));
            script.extend(int(key));
        }
        script.extend(int(20));
        script.extend(op(OpCode::PACKMAP));
        script.extend(pushes(n));
        script
    });
    // An array holding a map holding a struct holding an array.
    assert_boundary("array, map, struct, array", 1543, |n| {
        [
            new_array(500),
            int(1),
            op(OpCode::PACKSTRUCT),
            int(0),
            int(1),
            op(OpCode::PACKMAP),
            int(1),
            op(OpCode::PACK),
            pushes(n),
        ]
        .concat()
    });
    assert_eq!(
        assert_same_state(
            &[arrays(21), int(21), op(OpCode::PACK)].concat(),
            "21 arrays of 100"
        ),
        VmState::Fault
    );
}

#[test]
fn item_count_boundary_matches_canonical_for_slots() {
    assert_boundary("locals", 1793, |n| [init_slot(255, 0), pushes(n)].concat());
    assert_boundary("static fields", 1793, |n| {
        [init_static_slot(255), pushes(n)].concat()
    });
    assert_boundary("arguments", 2038, |n| {
        [pushes(10), init_slot(0, 10), pushes(n)].concat()
    });
    // The slots of a suspended frame still count while the callee runs.
    assert_boundary("locals of two frames", 1538, |n| {
        [
            init_slot(255, 0),
            vec![OpCode::CALL.byte(), 3],
            op(OpCode::RET),
            init_slot(255, 0),
            pushes(n),
            op(OpCode::RET),
        ]
        .concat()
    });
}

/// `NEWARRAY0` wrapped in `levels - 1` further arrays.
fn array_chain(levels: usize) -> Vec<u8> {
    [
        op(OpCode::NEWARRAY0),
        [int(1), op(OpCode::PACK)].concat().repeat(levels - 1),
    ]
    .concat()
}

#[test]
fn nesting_up_to_the_limit_matches_canonical() {
    on_large_stack(|| {
        for levels in [
            1,
            2,
            MAX_COMPOUND_NESTING_DEPTH - 1,
            MAX_COMPOUND_NESTING_DEPTH,
        ] {
            assert_eq!(
                assert_same_state(&array_chain(levels), &format!("{levels} levels")),
                VmState::Halt
            );
        }
        // Past its own item limit the canonical engine faults as well.
        assert_eq!(
            assert_same_state(&array_chain(2049), "2049 levels"),
            VmState::Fault
        );
    });
}

/// The depths at which the interpreter used to be stricter than the canonical
/// engine. Both now reach the same state there: the interpreter's nesting limit
/// is `MaxStackSize`, and the canonical count ends at the same depth.
#[test]
fn nesting_depths_the_interpreter_used_to_reject_now_match_canonical() {
    on_large_stack(|| {
        for levels in [65, 1000, MAX_COMPOUND_NESTING_DEPTH] {
            assert_eq!(
                assert_same_state(&array_chain(levels), &format!("{levels} levels")),
                VmState::Halt,
                "{levels} levels"
            );
        }
    });
}

/// Shapes whose count changes as a value moves between the stack, the slots and
/// a collection. The canonical state is pinned here, and the interpreter must
/// reach it: it counts a stored compound, a removed child and a copied struct
/// the way neo-vm v3.10.1 does.
#[test]
fn shapes_shared_between_the_engines_reach_the_canonical_state() {
    // A chain grown from the inside, short enough to stay under the item
    // limit: one reference a link.
    const CHAIN_LINKS: usize = 60;

    let append_array = [
        op(OpCode::NEWARRAY0),
        op(OpCode::DUP),
        new_array(100),
        op(OpCode::APPEND),
    ]
    .concat();
    let cases: Vec<(&str, VmState, Vec<u8>)> = vec![
        // The stored array stays counted for as long as its parent is on the
        // stack: 102, 502 and 503 references a round in the next three.
        (
            "array appended to an array",
            VmState::Fault,
            [append_array.repeat(30), pushes(1900)].concat(),
        ),
        (
            "array stored with SETITEM",
            VmState::Fault,
            [
                [
                    new_array(1),
                    op(OpCode::DUP),
                    int(0),
                    new_array(500),
                    op(OpCode::SETITEM),
                ]
                .concat()
                .repeat(10),
                pushes(2000),
            ]
            .concat(),
        ),
        (
            "array stored in a map",
            VmState::Fault,
            [
                [
                    op(OpCode::NEWMAP),
                    op(OpCode::DUP),
                    int(0),
                    new_array(500),
                    op(OpCode::SETITEM),
                ]
                .concat()
                .repeat(10),
                pushes(2000),
            ]
            .concat(),
        ),
        // Two references a round, and 1400 pushes on top of them.
        (
            "buffer appended to an array",
            VmState::Fault,
            [
                [
                    op(OpCode::NEWARRAY0),
                    op(OpCode::DUP),
                    int(4),
                    op(OpCode::NEWBUFFER),
                    op(OpCode::APPEND),
                ]
                .concat()
                .repeat(600),
                pushes(1400),
            ]
            .concat(),
        ),
        // Each round leaves one array on the stack; the reference its parent
        // held goes when the parent is released.
        (
            "parent released with its appended child",
            VmState::Halt,
            [
                op(OpCode::NEWARRAY0),
                op(OpCode::NEWARRAY0),
                op(OpCode::OVER),
                op(OpCode::OVER),
                op(OpCode::APPEND),
                op(OpCode::NIP),
            ]
            .concat()
            .repeat(1500),
        ),
        // A chain grown from the inside counts one reference a link.
        (
            "chain grown by APPEND",
            VmState::Halt,
            [
                op(OpCode::NEWARRAY0),
                op(OpCode::DUP),
                [op(OpCode::NEWARRAY0), op(OpCode::TUCK), op(OpCode::APPEND)]
                    .concat()
                    .repeat(CHAIN_LINKS),
            ]
            .concat(),
        ),
        // The canonical engine shares one struct where the interpreter
        // stores a copy per reference.
        (
            "one struct packed many times",
            VmState::Halt,
            [
                [int(100), op(OpCode::NEWSTRUCT)].concat(),
                op(OpCode::DUP).repeat(20),
                int(21),
                op(OpCode::PACK),
                pushes(1900),
            ]
            .concat(),
        ),
        // The array holds a copy of the struct, 102 references a round.
        (
            "struct appended to an array",
            VmState::Fault,
            [
                [
                    op(OpCode::NEWARRAY0),
                    op(OpCode::DUP),
                    [int(100), op(OpCode::NEWSTRUCT)].concat(),
                    op(OpCode::APPEND),
                ]
                .concat()
                .repeat(30),
                pushes(1900),
            ]
            .concat(),
        ),
        // A callee that only sees its own frame.
        (
            "array held by a suspended frame",
            VmState::Halt,
            [
                init_slot(1, 0),
                new_array(1000),
                op(OpCode::STLOC0),
                vec![OpCode::CALL.byte(), 3],
                op(OpCode::RET),
                pushes(1040),
                op(OpCode::RET),
            ]
            .concat(),
        ),
        // REMOVE gives back the removed array and its 300 children, which
        // leaves one reference a round.
        (
            "children removed from a packed array",
            VmState::Halt,
            [
                [
                    new_array(300),
                    int(1),
                    op(OpCode::PACK),
                    op(OpCode::DUP),
                    int(0),
                    op(OpCode::REMOVE),
                ]
                .concat()
                .repeat(6),
                pushes(2000),
            ]
            .concat(),
        ),
    ];

    for (label, canonical, script) in cases {
        assert_eq!(run_canonical(&script), canonical, "{label}");
        assert_eq!(run_fast(&script).state(), canonical, "{label}");
    }
}

/// Hands the interpreter two arrays that may be one object on the host side.
struct SharingHost {
    full_stack: bool,
}

impl SyscallProvider for SharingHost {
    fn syscall(
        &mut self,
        _api: u32,
        _ip: usize,
        stack: &mut Vec<StackValue>,
    ) -> Result<(), String> {
        stack.extend(one_array_twice());
        Ok(())
    }

    fn needs_full_stack(&self) -> bool {
        self.full_stack
    }
}

fn one_array_twice() -> Vec<StackValue> {
    vec![StackValue::Array(vec![StackValue::Integer(1); 1500]); 2]
}

/// A host can hand the interpreter two arrays that are one object on its side.
///
/// ABI values carry no identity, so the interpreter counts the two as two
/// compounds with 1500 children each. That is exactly what the canonical engine
/// sees once `external_vm::external_to_canonical` builds one item per reference
/// in the stack it is given, so both engines fault on the same item count.
#[test]
fn compounds_from_the_host_are_counted_once_per_reference() {
    let single = run_fast_with(
        &pushes(10),
        vec![one_array_twice()[0].clone()],
        &mut PureHost,
    );
    assert_eq!(single, Fast::Halt, "one array of 1500");

    let initial = run_fast_with(&pushes(10), one_array_twice(), &mut PureHost);
    assert_eq!(
        initial,
        Fast::Fault(ITEM_COUNT_FAULT.to_string()),
        "two arrays of 1500 on the initial stack"
    );

    // `System.Runtime.Notify`, which declares two arguments.
    let mut syscall = [pushes(2), op(OpCode::SYSCALL)].concat();
    syscall.extend(0x616f_0195u32.to_le_bytes());
    let callt = [pushes(2), vec![OpCode::CALLT.byte(), 7, 0]].concat();
    for (label, call, full_stack) in [
        ("syscall", &syscall, false),
        ("full-stack syscall", &syscall, true),
        ("CALLT", &callt, false),
    ] {
        let script = [call.clone(), pushes(10)].concat();
        let outcome = run_fast_with(&script, Vec::new(), &mut SharingHost { full_stack });
        assert_eq!(
            outcome,
            Fast::Fault(ITEM_COUNT_FAULT.to_string()),
            "{label}"
        );
    }
}

/// Deterministic generator for the randomized comparison.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Primitive,
    Array,
    Struct,
    Map,
}

/// What the generator knows about a value it has put on the stack.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Shape {
    kind: Kind,
    /// Whether one of its direct children is a struct.
    struct_child: bool,
    /// Every object reachable inside it, as a set of object numbers, this
    /// value's own number included. Copies of one collection hold the same
    /// bit, and every primitive holds none.
    inside: u128,
}

/// The bit that stands for an object number. Primitives share number 0 and
/// are not objects, so they have no bit.
const fn object_bit(object: usize) -> u128 {
    if object == 0 {
        0
    } else {
        1u128 << (object as u32 % u128::BITS)
    }
}

impl Shape {
    const PRIMITIVE: Self = Self::childless(Kind::Primitive, 0);

    const fn childless(kind: Kind, object: usize) -> Self {
        Self {
            kind,
            struct_child: false,
            inside: object_bit(object),
        }
    }

    fn holding(kind: Kind, object: usize, children: &[Self]) -> Self {
        Self {
            kind,
            struct_child: children.iter().any(|child| child.kind == Kind::Struct),
            inside: children
                .iter()
                .fold(object_bit(object), |inside, child| inside | child.inside),
        }
    }

    /// A copy of this value under a new object number, as CONVERT and VALUES
    /// make one.
    fn derived(self, kind: Kind, object: usize) -> Self {
        Self {
            kind,
            inside: self.inside | object_bit(object),
            ..self
        }
    }

    fn is(self, kinds: &[Kind]) -> bool {
        kinds.contains(&self.kind)
    }

    /// Whether `value` can be stored into this collection.
    ///
    /// The canonical engine shares one object where the interpreter keeps a
    /// copy of a value, and it propagates a mutation through every alias.
    /// Storing a value that already holds this collection would close an
    /// aliasing cycle the interpreter cannot represent, so the generator does
    /// not emit it: everything it emits is an acyclic tree of aliases, which
    /// is the shape both engines count alike.
    ///
    /// A collection stored into itself is excluded for the same reason: the
    /// canonical engine used not to return from that instruction. It does now
    /// (task 52), which `reference_counter_v3_10_1.rs` covers; the generator
    /// is left as it was.
    fn can_store(self, kinds: &[Kind], value: Self) -> bool {
        self.is(kinds) && (self.inside & value.inside) == 0
    }
}

const SEQUENCES: &[Kind] = &[Kind::Array, Kind::Struct];
const COLLECTIONS: &[Kind] = &[Kind::Array, Kind::Struct, Kind::Map];

/// Emits a script of collection operations that mostly type-check.
///
/// `mutating` adds the operations that move compounds between the stack, the
/// slots and a collection: storing one compound into another, removing
/// children, and the operations that copy a struct (PACK and PACKSTRUCT here,
/// VALUES in the canonical engine).
fn random_script(rng: &mut Rng, mutating: bool) -> Vec<u8> {
    let mut script = [init_static_slot(2), init_slot(3, 0)].concat();
    let mut stack: Vec<Shape> = Vec::new();
    let mut locals = [Shape::PRIMITIVE; 3];
    let mut emit = |parts: &[Vec<u8>]| script.extend(parts.concat());
    let mut objects = 0;
    let mut fresh = || {
        objects += 1;
        objects
    };

    for _ in 0..8 + rng.below(40) {
        let top = stack.last().copied();
        let below = stack.len().checked_sub(2).map(|index| stack[index]);
        match (rng.below(if mutating { 22 } else { 17 }), top) {
            (0, _) => {
                let count = 1 + rng.below(300);
                emit(&[pushes(count)]);
                stack.extend(std::iter::repeat_n(Shape::PRIMITIVE, count));
            }
            (1 | 2, _) => {
                emit(&[new_array(rng.below(700))]);
                stack.push(Shape::childless(Kind::Array, fresh()));
            }
            (3, _) => {
                emit(&[int(rng.below(700)), op(OpCode::NEWSTRUCT)]);
                stack.push(Shape::childless(Kind::Struct, fresh()));
            }
            (4, _) => {
                emit(&[op(OpCode::NEWMAP)]);
                stack.push(Shape::childless(Kind::Map, fresh()));
            }
            (5 | 6, Some(top)) => {
                let copies = 1 + rng.below(40);
                emit(&[op(OpCode::DUP).repeat(copies)]);
                stack.extend(std::iter::repeat_n(top, copies));
            }
            (7, Some(_)) => {
                emit(&[op(OpCode::DROP)]);
                stack.pop();
            }
            (pack @ 8..=10, Some(_)) => {
                let count = 1 + rng.below(stack.len().min(6));
                let (opcode, kind) = if pack == 10 {
                    (OpCode::PACKSTRUCT, Kind::Struct)
                } else {
                    (OpCode::PACK, Kind::Array)
                };
                let packed = Shape::holding(kind, fresh(), &stack[stack.len() - count..]);
                if mutating || !packed.struct_child {
                    emit(&[int(count), op(opcode)]);
                    stack.truncate(stack.len() - count);
                    stack.push(packed);
                }
            }
            (11, Some(top)) => {
                // Wraps the value on top in a map with one entry.
                emit(&[int(rng.below(4)), int(1), op(OpCode::PACKMAP)]);
                stack.pop();
                stack.push(Shape::holding(Kind::Map, fresh(), &[top]));
            }
            (12, Some(top)) => {
                let slot = rng.below(3);
                emit(&[
                    op(OpCode::DUP),
                    vec![OpCode::STLOC0.byte() + slot as u8],
                    op(OpCode::DUP),
                    vec![OpCode::STSFLD0.byte() + slot.min(1) as u8],
                ]);
                locals[slot] = top;
            }
            (13, _) => {
                let slot = rng.below(3);
                emit(&[vec![OpCode::LDLOC0.byte() + slot as u8]]);
                stack.push(locals[slot]);
            }
            (14, Some(Shape::PRIMITIVE)) if below.is_some_and(|below| below.is(SEQUENCES)) => {
                // Appends the primitive on top to the collection below it.
                emit(&[op(OpCode::OVER), op(OpCode::SWAP), op(OpCode::APPEND)]);
                stack.pop();
            }
            (15, Some(top)) if top.is(SEQUENCES) => {
                let (target, kind) = if top.kind == Kind::Array {
                    (STRUCT_TYPE, Kind::Struct)
                } else {
                    (ARRAY_TYPE, Kind::Array)
                };
                emit(&[op(OpCode::DUP), vec![OpCode::CONVERT.byte(), target]]);
                stack.push(top.derived(kind, fresh()));
            }
            (16, Some(top)) if top.is(COLLECTIONS) && (mutating || !top.struct_child) => {
                emit(&[op(OpCode::DUP), op(OpCode::VALUES)]);
                stack.push(top.derived(Kind::Array, fresh()));
            }
            (17, Some(top)) if below.is_some_and(|below| below.can_store(SEQUENCES, top)) => {
                emit(&[op(OpCode::OVER), op(OpCode::SWAP), op(OpCode::APPEND)]);
                stack.pop();
                if let (Some(parent), Kind::Struct) = (stack.last_mut(), top.kind) {
                    parent.struct_child = true;
                }
            }
            (18, Some(top)) if below.is_some_and(|below| below.can_store(COLLECTIONS, top)) => {
                emit(&[
                    op(OpCode::OVER),
                    op(OpCode::SWAP),
                    int(0),
                    op(OpCode::SWAP),
                    op(OpCode::SETITEM),
                ]);
                stack.pop();
                if let (Some(parent), Kind::Struct) = (stack.last_mut(), top.kind) {
                    parent.struct_child = true;
                }
            }
            (19, Some(top)) if top.is(SEQUENCES) => {
                emit(&[op(OpCode::DUP), int(0), op(OpCode::REMOVE)]);
            }
            (20, Some(top)) if top.is(SEQUENCES) => {
                emit(&[op(OpCode::DUP), op(OpCode::POPITEM)]);
                stack.push(Shape::PRIMITIVE);
            }
            (21, Some(top)) if top.is(COLLECTIONS) => {
                emit(&[op(OpCode::DUP), op(OpCode::CLEARITEMS)]);
            }
            _ => {
                emit(&[pushes(1)]);
                stack.push(Shape::PRIMITIVE);
            }
        }
    }
    script
}

/// Compares the engines on generated scripts and returns how many of them
/// the interpreter faulted on the item count.
fn compare_random_scripts(seed: u64, mutating: bool) -> usize {
    let mut rng = Rng(seed);
    let mut item_count_faults = 0;
    for case in 0..4000 {
        let script = random_script(&mut rng, mutating);
        let fast = run_fast(&script);
        let canonical = run_canonical(&script);
        assert_eq!(
            fast.state(),
            canonical,
            "lightweight {fast:?}, case {case}, script {script:02x?}"
        );
        if fast == Fast::Fault(ITEM_COUNT_FAULT.to_string()) {
            item_count_faults += 1;
        }
    }
    item_count_faults
}

#[test]
fn random_collection_scripts_reach_the_canonical_state() {
    let mut item_count_faults = 0;
    for seed in [
        0x9e37_79b9_7f4a_7c15u64,
        0x0123_4567_89ab_cdef,
        0xfedc_ba98_7654_3210,
    ] {
        item_count_faults += compare_random_scripts(seed, false);
    }
    assert!(
        item_count_faults > 100,
        "only {item_count_faults} generated scripts reached the item limit"
    );
}

#[test]
fn random_mutating_scripts_reach_the_canonical_state() {
    let mut item_count_faults = 0;
    for seed in [
        0xd1b5_4a32_d192_ed03u64,
        0x2545_f491_4f6c_dd1d,
        0xdead_beef_cafe_babe,
    ] {
        item_count_faults += compare_random_scripts(seed, true);
    }
    assert!(
        item_count_faults > 100,
        "only {item_count_faults} generated scripts reached the item limit"
    );
}
