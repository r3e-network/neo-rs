use super::abi::abi_nesting_exceeds;
use super::propagate::{contains_compound_id, replace_alias};
use super::*;
use alloc::vec;

#[test]
fn discarding_abi_values_does_not_recurse_on_nesting_depth() {
    const DEPTH: usize = 100_000;
    // Far too small for one native frame per nesting level.
    const THREAD_STACK_BYTES: usize = 256 * 1024;

    let nested = |wrap: fn(AbiStackValue) -> AbiStackValue| {
        let mut value = AbiStackValue::ByteString(vec![0xaa; 8]);
        for _ in 0..DEPTH {
            value = wrap(value);
        }
        value
    };
    let values = vec![
        nested(|inner| AbiStackValue::Array(vec![inner])),
        nested(|inner| AbiStackValue::Struct(vec![inner])),
        nested(|inner| AbiStackValue::Map(vec![(AbiStackValue::Integer(0), inner)])),
        nested(|inner| AbiStackValue::Map(vec![(inner, AbiStackValue::Integer(0))])),
        AbiStackValue::Buffer(vec![0xbb; 8]),
    ];

    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(move || discard_abi_values(values))
        .expect("spawn small-stack thread")
        .join()
        .expect("release completes");
}

/// A single-root chain `levels` deep that mixes every compound kind.
fn nested(levels: usize) -> StackValue {
    let mut value = StackValue::Struct(0, Vec::new());
    let mut next_id = 1u64;
    for level in 1..levels {
        value = match level % 3 {
            0 => {
                next_id += 1;
                StackValue::Array(next_id, vec![value])
            }
            1 => {
                next_id += 1;
                StackValue::Struct(next_id, vec![value])
            }
            _ => {
                next_id += 1;
                StackValue::Map(next_id, vec![(StackValue::Integer(0), value)])
            }
        };
    }
    value
}

/// Every walk over a legal deepest value has to fit a quarter of the node's
/// stack, because none of them recurses any more.
#[test]
fn every_walk_over_a_maximum_depth_value_fits_a_small_stack() {
    const THREAD_STACK_BYTES: usize = 256 * 1024;

    std::thread::Builder::new()
        .stack_size(THREAD_STACK_BYTES)
        .spawn(|| {
            let levels = MAX_COMPOUND_NESTING_DEPTH;
            let value = nested(levels);

            // One entry per level, plus a second item for every map entry,
            // which the canonical counter also counts twice.
            let maps = (1..levels).filter(|level| level % 3 == 2).count();
            let items = 1 + (levels - 1) + maps;
            assert!(items > MAX_COMPOUND_NESTING_DEPTH);
            assert_eq!(
                counted_items([core::slice::from_ref(&value)], usize::MAX),
                items
            );
            assert!(
                counted_items([core::slice::from_ref(&value)], MAX_COMPOUND_NESTING_DEPTH)
                    > MAX_COMPOUND_NESTING_DEPTH
            );
            assert!(!nesting_exceeds(&value, MAX_COMPOUND_NESTING_DEPTH));
            assert!(nesting_exceeds(&value, MAX_COMPOUND_NESTING_DEPTH - 1));
            assert!(contains_compound_id(&value, 0));
            assert_eq!(
                find_affected_indices(0, core::slice::from_ref(&value)),
                vec![0]
            );

            let copy = value.clone();
            assert_eq!(copy, value);
            assert!(structurally_equal(&copy, &value));
            assert!(!format!("{value:?}").is_empty());

            let mut ids = CompoundIds::default();
            let cloned = ids.deep_clone(&value);
            assert!(structurally_equal(&cloned, &value));
            assert_ne!(compound_id(&cloned), compound_id(&value));

            let abi = to_abi_value(&value);
            assert!(!abi_nesting_exceeds(&abi, levels));
            let imported = ids.import_abi(abi);
            assert!(structurally_equal(&imported, &value));

            // Alias replacement touches every level and must not recurse.
            let mut target = value.clone();
            let replacement = StackValue::Array(999, vec![StackValue::Null]);
            assert!(replace_alias(&mut target, &replacement, levels));

            // Dropping the whole set happens on this small stack too.
            drop((value, copy, cloned, imported, target));
        })
        .expect("spawn small-stack thread")
        .join()
        .expect("every walk fits a 256 KiB stack");
}

#[test]
fn counted_items_matches_the_number_of_references() {
    let mut ids = CompoundIds::default();
    let inner = ids.array(vec![StackValue::Integer(1), StackValue::Integer(2)]);
    let shared = ids.array(vec![inner.clone(), inner.clone(), StackValue::Null]);
    // Three root entries, the outer array contributes three children, the
    // shared inner array contributes two, and the duplicate alias adds
    // nothing because its id is already counted.
    let roots = vec![shared.clone(), shared, StackValue::Null];
    assert_eq!(
        counted_items([roots.as_slice()], MAX_COMPOUND_NESTING_DEPTH),
        3 + 3 + 2
    );
}

#[test]
fn counted_items_counts_suspended_slot_entries_too() {
    let mut ids = CompoundIds::default();
    let value = ids.array(vec![StackValue::Integer(1), StackValue::Integer(2)]);
    let slots = vec![value.clone(), value, StackValue::Null];
    assert_eq!(
        counted_items([&[][..], slots.as_slice(),], MAX_COMPOUND_NESTING_DEPTH),
        3 + 2
    );
}
