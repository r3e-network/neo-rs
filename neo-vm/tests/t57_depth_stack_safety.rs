//! Stack-safety regression for the canonical (spec) engine's stack-item
//! traversals.
//!
//! A single-root chain nested `MaxStackSize` (2048) deep is a legal value, so
//! every traversal of a stack item has to run on an explicit stack: one native
//! frame per nesting level overflowed a 1 MiB thread stack (Debug formatting and
//! the `StackItem` -> `StackValue` conversion even overflowed 2 MiB).
//!
//! Each test builds such a chain, runs exactly one traversal on a fresh 1 MiB
//! thread and asserts the result the recursive form produced. Chains are built
//! and dropped on a 256 MiB helper thread, so only the traversal under test is
//! measured. Both profiles must pass:
//!
//! ```text
//! cargo test -p neo-vm --test t57_depth_stack_safety
//! cargo test -p neo-vm --release --test t57_depth_stack_safety
//! ```

use neo_vm::{
    ExecutionEngine, ExecutionEngineLimits, JsonSerializer, OpCode, Script, StackItem, StackValue,
    VmOrderedDictionary, VmState, stack_item_rpc_json,
};
use std::thread;

const MIB: usize = 1024 * 1024;

/// Depth of the hand-built chains: `ExecutionEngineLimits::DEFAULT`'s
/// `max_stack_size`, i.e. the deepest legal single-root nesting.
const LEVELS: usize = 2048;

/// Rounds of `append_chain` / `struct_chain`. Each round adds one level; the
/// engine's reference counter admits one item less than the chain length, so
/// 2045 rounds is the deepest chain that still executes (`MaxStackSize` is
/// 2048 and the two roots on the stack need slots of their own).
const CHAIN_ROUNDS: usize = 2045;

// ---------------------------------------------------------------------------
// Thread helpers
// ---------------------------------------------------------------------------

/// Runs `body` on a thread with a 1 MiB stack: the stack size the traversals
/// have to survive.
fn on_1mib<R: Send + 'static>(body: impl FnOnce() -> R + Send + 'static) -> R {
    thread::Builder::new()
        .stack_size(MIB)
        .spawn(body)
        .expect("spawn a 1 MiB thread")
        .join()
        .expect("the 1 MiB thread panicked")
}

/// Runs `body` on a 256 MiB thread, used for building and dropping the chains.
fn on_big_stack<R: Send + 'static>(body: impl FnOnce() -> R + Send + 'static) -> R {
    thread::Builder::new()
        .stack_size(256 * MIB)
        .spawn(body)
        .expect("spawn a 256 MiB thread")
        .join()
        .expect("the 256 MiB thread panicked")
}

/// Drops `value` on a 256 MiB thread so that the drop is not part of the
/// 1 MiB measurement.
fn drop_later<T: Send + 'static>(value: T) {
    on_big_stack(move || drop(value));
}

// ---------------------------------------------------------------------------
// Fixtures
// ---------------------------------------------------------------------------

fn nested_arrays(levels: usize) -> StackItem {
    let mut item = StackItem::from_array(Vec::new());
    for _ in 1..levels {
        item = StackItem::from_array(vec![item]);
    }
    item
}

fn nested_structs(levels: usize) -> StackItem {
    let mut item = StackItem::from_struct(Vec::new());
    for _ in 1..levels {
        item = StackItem::from_struct(vec![item]);
    }
    item
}

fn nested_maps(levels: usize) -> StackItem {
    let mut item = StackItem::from_map(VmOrderedDictionary::new());
    for _ in 1..levels {
        let mut map = VmOrderedDictionary::new();
        map.insert(StackItem::from_i64(0), item);
        item = StackItem::from_map(map);
    }
    item
}

fn array_levels(item: &StackItem) -> usize {
    let mut levels = 0;
    let mut current = item.clone();
    while let StackItem::Array(array) = current {
        levels += 1;
        match array.get(0) {
            Some(child) => current = child,
            None => break,
        }
    }
    levels
}

fn struct_levels(item: &StackItem) -> usize {
    let mut levels = 0;
    let mut current = item.clone();
    while let StackItem::Struct(structure) = current {
        levels += 1;
        match structure.get(0) {
            Ok(child) => current = child,
            Err(_) => break,
        }
    }
    levels
}

fn map_levels(item: &StackItem) -> usize {
    let mut levels = 0;
    let mut current = item.clone();
    while let StackItem::Map(map) = current {
        levels += 1;
        match map.iter().next() {
            Some((_key, value)) => current = value,
            None => break,
        }
    }
    levels
}

/// `NEWARRAY0 DUP (NEWARRAY0 TUCK APPEND)*rounds`: a one-element array chain.
fn append_chain(rounds: usize) -> Vec<u8> {
    let mut script = vec![OpCode::NEWARRAY0.byte(), OpCode::DUP.byte()];
    for _ in 0..rounds {
        script.extend([
            OpCode::NEWARRAY0.byte(),
            OpCode::TUCK.byte(),
            OpCode::APPEND.byte(),
        ]);
    }
    script
}

/// `NEWSTRUCT0 (NEWSTRUCT0 TUCK SWAP APPEND)*rounds`: a struct built inside
/// out; every `APPEND` clones the struct collected so far.
fn struct_chain(rounds: usize) -> Vec<u8> {
    let mut script = vec![OpCode::NEWSTRUCT0.byte()];
    for _ in 0..rounds {
        script.extend([
            OpCode::NEWSTRUCT0.byte(),
            OpCode::TUCK.byte(),
            OpCode::SWAP.byte(),
            OpCode::APPEND.byte(),
        ]);
    }
    script
}

fn engine_for(script: Vec<u8>) -> ExecutionEngine {
    let mut engine = ExecutionEngine::new(None);
    engine.set_limits(ExecutionEngineLimits::DEFAULT);
    engine
        .load_script(Script::new_relaxed(script), -1, 0)
        .expect("load script");
    engine
}

// ---------------------------------------------------------------------------
// Engine execution (array APPEND, struct APPEND + struct clone budget)
// ---------------------------------------------------------------------------

#[test]
fn array_chain_executes_on_1mib_stack() {
    let script = append_chain(CHAIN_ROUNDS);
    let (state, count) = on_1mib(move || {
        let mut engine = engine_for(script);
        let state = engine.execute();
        let count = engine.reference_counter().count();
        // The engine (evaluation stack, result stack, roots) is dropped here,
        // on the 1 MiB thread.
        (state, count)
    });
    assert_eq!(state, VmState::Halt);
    assert!(
        count <= LEVELS,
        "reference count {count} above MaxStackSize {LEVELS}"
    );
}

#[test]
fn struct_chain_executes_on_1mib_stack() {
    let script = struct_chain(CHAIN_ROUNDS);
    let (state, count) = on_1mib(move || {
        let mut engine = engine_for(script);
        let state = engine.execute();
        let count = engine.reference_counter().count();
        (state, count)
    });
    assert_eq!(state, VmState::Halt);
    assert!(
        count <= LEVELS,
        "reference count {count} above MaxStackSize {LEVELS}"
    );
}

// ---------------------------------------------------------------------------
// Struct clone (`Struct::clone_with_limits`, the budgeted Clone of the
// reference: `Queue<Struct>` + `limits--`)
// ---------------------------------------------------------------------------

#[test]
fn struct_clone_with_limits_at_max_depth_on_1mib_stack() {
    let limits = ExecutionEngineLimits::DEFAULT;
    let tight = ExecutionEngineLimits {
        max_stack_size: LEVELS as u32 - 2,
        ..ExecutionEngineLimits::DEFAULT
    };
    let structure = on_big_stack(|| nested_structs(LEVELS));

    let (structure, clone, levels, exhausted) = on_1mib(move || {
        let cloned = match &structure {
            StackItem::Struct(inner) => inner.clone_with_limits(&limits).expect("clone in budget"),
            other => panic!("expected a struct, got {other:?}"),
        };
        let levels = struct_levels(&StackItem::Struct(cloned.clone()));
        let exhausted = match &structure {
            StackItem::Struct(inner) => inner.clone_with_limits(&tight).is_err(),
            other => panic!("expected a struct, got {other:?}"),
        };
        (structure, cloned, levels, exhausted)
    });

    assert_eq!(levels, LEVELS);
    // One sub item per level, one budget unit per sub item: a limit one unit
    // below the depth has to be refused, exactly like the reference `limits--`.
    assert!(exhausted, "a struct deeper than the clone budget must fail");
    drop_later((structure, clone));
}

// ---------------------------------------------------------------------------
// deep_clone
// ---------------------------------------------------------------------------

#[test]
fn deep_clone_at_max_depth_on_1mib_stack() {
    let (array, structure, map) =
        on_big_stack(|| (nested_arrays(LEVELS), nested_structs(LEVELS), nested_maps(LEVELS)));

    let (values, clones, verdicts) = on_1mib(move || {
        let array_clone = array.deep_clone();
        let struct_clone = structure.deep_clone();
        let map_clone = map.deep_clone();
        let verdicts = (
            array == array_clone,
            structure == struct_clone,
            map == map_clone,
            array_levels(&array_clone),
            struct_levels(&struct_clone),
            map_levels(&map_clone),
        );
        ((array, structure, map), (array_clone, struct_clone, map_clone), verdicts)
    });

    assert_eq!(verdicts, (true, true, true, LEVELS, LEVELS, LEVELS));
    drop_later((values, clones));
}

// ---------------------------------------------------------------------------
// Struct limited equality (`Struct.Equals(StackItem, ExecutionLimits)`)
// ---------------------------------------------------------------------------

#[test]
fn struct_equals_with_limits_at_max_depth_on_1mib_stack() {
    let limits = ExecutionEngineLimits::DEFAULT;
    let tight = ExecutionEngineLimits {
        max_stack_size: LEVELS as u32 / 2,
        ..ExecutionEngineLimits::DEFAULT
    };
    let (structure, shallow) =
        on_big_stack(|| (nested_structs(LEVELS), nested_structs(LEVELS / 2)));

    let (values, verdicts) = on_1mib(move || {
        let clone = structure.deep_clone();
        let same = structure
            .equals_with_limits(&clone, &limits)
            .expect("same-depth comparison");
        let different = structure
            .equals_with_limits(&shallow, &limits)
            .expect("shallow comparison");
        let exhausted = structure
            .equals_with_limits(&clone, &tight)
            .map(|_| ())
            .map_err(|error| error.to_string());
        (
            (structure, clone, shallow),
            (same, different, exhausted),
        )
    });

    let (same, different, exhausted) = verdicts;
    assert!(same, "a struct must equal its own deep clone");
    assert!(!different, "structs of different depth must not compare equal");
    // One `MaxStackSize` unit per compared sub item, as in the reference.
    let exhausted = exhausted.expect_err("a comparison above the budget must fail");
    assert!(
        exhausted.contains("struct comparison"),
        "unexpected budget error: {exhausted}"
    );
    drop_later(values);
}

// ---------------------------------------------------------------------------
// `PartialEq` (`equals_with_refs`)
// ---------------------------------------------------------------------------

#[test]
fn partial_eq_at_max_depth_on_1mib_stack() {
    let (array, map, shallow_array, shallow_map) = on_big_stack(|| {
        (
            nested_arrays(LEVELS),
            nested_maps(LEVELS),
            nested_arrays(LEVELS / 2),
            nested_maps(LEVELS / 2),
        )
    });

    let (values, clones, verdicts) = on_1mib(move || {
        let array_clone = array.deep_clone();
        let map_clone = map.deep_clone();
        let verdicts = (
            array == array_clone,
            map == map_clone,
            array == shallow_array,
            map == shallow_map,
        );
        (
            (array, shallow_array),
            (array_clone, map_clone),
            verdicts,
        )
    });

    assert_eq!(verdicts, (true, true, false, false));
    drop_later((values, clones));
}

// ---------------------------------------------------------------------------
// Debug
// ---------------------------------------------------------------------------

#[test]
fn debug_compact_at_max_depth_on_1mib_stack() {
    let (array, structure, map) =
        on_big_stack(|| (nested_arrays(LEVELS), nested_structs(LEVELS), nested_maps(LEVELS)));

    let (values, texts) = on_1mib(move || {
        let texts = (
            format!("{array:?}"),
            format!("{structure:?}"),
            format!("{map:?}"),
        );
        ((array, structure, map), texts)
    });

    let (array_text, struct_text, map_text) = texts;
    // One `Type(Type {` wrapper per nesting level, and exactly one empty leaf.
    assert_eq!(array_text.matches("Array(Array {").count(), LEVELS);
    assert_eq!(array_text.matches("ArrayInner {").count(), LEVELS);
    assert_eq!(array_text.matches("items: []").count(), 1);
    assert_eq!(struct_text.matches("Struct(Struct {").count(), LEVELS);
    assert_eq!(struct_text.matches("StructInner {").count(), LEVELS);
    assert_eq!(struct_text.matches("items: []").count(), 1);
    assert_eq!(map_text.matches("Map(Map {").count(), LEVELS);
    assert_eq!(map_text.matches("MapInner {").count(), LEVELS);
    assert_eq!(map_text.matches("items: {}").count(), 1);
    drop_later(values);
}

#[test]
fn debug_pretty_at_depth_on_1mib_stack() {
    // `{:#?}` grows quadratically with depth (every level re-indents the lines
    // below it), so the alternate form is checked at a depth that keeps the
    // text small while still nesting far beyond any recursive formatter.
    let levels = 256;
    let array = on_big_stack(move || nested_arrays(levels));
    let (array, text) = on_1mib(move || {
        let text = format!("{array:#?}");
        (array, text)
    });

    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines.len() >= 12 * levels,
        "pretty output has only {} lines for {levels} levels",
        lines.len()
    );
    let leaf = lines
        .iter()
        .find(|line| line.trim() == "items: [],")
        .expect("the empty leaf is rendered");
    let indentation = leaf.len() - leaf.trim_start().len();
    assert_eq!(indentation % 4, 0, "indentation is a multiple of four");
    assert!(
        indentation >= 4 * (levels - 1),
        "the leaf is indented {indentation} spaces, expected at least {}",
        4 * (levels - 1)
    );
    drop_later(array);
}

// ---------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------

#[test]
fn json_serializer_at_max_depth_on_1mib_stack() {
    let array = on_big_stack(|| nested_arrays(LEVELS));
    let (array, value) = on_1mib(move || {
        let value = JsonSerializer::serialize_to_json(&array).expect("a nested array serializes");
        (array, value)
    });
    // `serde_json`'s own Display is recursive, so it runs on the big stack.
    let length = on_big_stack(move || {
        let text = value.to_string();
        drop(value);
        text.len()
    });
    // `[[[..]]]` with one bracket pair per level.
    assert_eq!(length, 2 * LEVELS);
    drop_later(array);
}

#[test]
fn rpc_json_at_max_depth_on_1mib_stack() {
    let array = on_big_stack(|| nested_arrays(LEVELS));
    let (array, value) = on_1mib(move || {
        let value = stack_item_rpc_json(&array, None).expect("a nested array renders");
        (array, value)
    });
    let types = on_big_stack(move || {
        let text = value.to_string();
        drop(value);
        text.matches("\"type\"").count()
    });
    assert_eq!(types, LEVELS, "one envelope per nesting level");

    let (array, error) = on_1mib(move || {
        let error = stack_item_rpc_json(&array, Some(4096))
            .expect_err("the size budget is exhausted long before the leaf")
            .to_string();
        (array, error)
    });
    assert!(error.contains("Max size reached"), "unexpected error: {error}");
    drop_later(array);
}

// ---------------------------------------------------------------------------
// StackItem <-> StackValue
// ---------------------------------------------------------------------------

#[test]
fn stack_value_roundtrip_at_max_depth_on_1mib_stack() {
    let array = on_big_stack(|| nested_arrays(LEVELS));
    let (array, value) = on_1mib(move || {
        let value = StackValue::try_from(array.clone()).expect("an array converts to StackValue");
        (array, value)
    });
    let round_tripped = on_1mib(move || {
        StackItem::try_from(value).expect("a StackValue array converts back to StackItem")
    });

    assert_eq!(array_levels(&round_tripped), LEVELS);
    drop_later((array, round_tripped));
}

// ---------------------------------------------------------------------------
// Hash
// ---------------------------------------------------------------------------

#[test]
fn hash_code_at_max_depth_on_1mib_stack() {
    let (array, shallow) = on_big_stack(|| (nested_arrays(LEVELS), nested_arrays(LEVELS / 2)));
    let (values, hashes) = on_1mib(move || {
        let hashes = (array.hash_code(), array.hash_code(), shallow.hash_code());
        ((array, shallow), hashes)
    });
    let (first, second, other) = hashes;
    assert_eq!(first, second, "hashing must be deterministic");
    assert_ne!(first, other, "different nesting must hash differently");
    drop_later(values);
}

// ---------------------------------------------------------------------------
// Drop
// ---------------------------------------------------------------------------

#[test]
fn drop_at_max_depth_on_1mib_stack() {
    let (array, structure, map) =
        on_big_stack(|| (nested_arrays(LEVELS), nested_structs(LEVELS), nested_maps(LEVELS)));
    on_1mib(move || drop((array, structure, map)));
}
