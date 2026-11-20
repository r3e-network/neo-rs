//! Stack-safety regression for the JSON-RPC stack-item renderer.
//!
//! A single-root chain nested `MaxStackSize` (2048) deep is a legal value, so
//! the renderer has to walk it on an explicit stack: one native frame per
//! nesting level overflowed a 1 MiB thread stack.
//!
//! The test builds such a chain, renders it on a fresh 1 MiB thread and asserts
//! the result the recursive form produced. The chain is built and dropped on a
//! 256 MiB helper thread, so only the render is measured. Both profiles must
//! pass:
//!
//! ```text
//! cargo test -p neo-rpc-client --test stack_item_json_depth
//! cargo test -p neo-rpc-client --release --test stack_item_json_depth
//! ```

use std::thread;

use neo_rpc_client::serialization::stack_item_json::stack_item_rpc_json;
use neo_vm::StackItem;

const MIB: usize = 1024 * 1024;

/// Depth of the hand-built chain: the deepest legal single-root nesting.
const LEVELS: usize = 2048;

/// Runs `body` on a thread with a 1 MiB stack: the stack size the renderer has
/// to survive.
fn on_1mib<R: Send + 'static>(body: impl FnOnce() -> R + Send + 'static) -> R {
    thread::Builder::new()
        .stack_size(MIB)
        .spawn(body)
        .expect("spawn a 1 MiB thread")
        .join()
        .expect("the 1 MiB thread panicked")
}

/// Runs `body` on a 256 MiB thread, used for building and dropping the chain.
fn on_big_stack<R: Send + 'static>(body: impl FnOnce() -> R + Send + 'static) -> R {
    thread::Builder::new()
        .stack_size(256 * MIB)
        .spawn(body)
        .expect("spawn a 256 MiB thread")
        .join()
        .expect("the 256 MiB thread panicked")
}

fn nested_arrays(levels: usize) -> StackItem {
    let mut item = StackItem::from_array(Vec::new());
    for _ in 1..levels {
        item = StackItem::from_array(vec![item]);
    }
    item
}

fn drop_later<T: Send + 'static>(value: T) {
    on_big_stack(move || drop(value));
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
    assert!(
        error.contains("Max size reached"),
        "unexpected error: {error}"
    );
    drop_later(array);
}
