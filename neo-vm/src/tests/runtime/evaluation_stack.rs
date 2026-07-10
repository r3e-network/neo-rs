use super::*;

#[test]
fn test_push_pop() {
    let reference_counter = ReferenceCounter::new();
    let mut stack = EvaluationStack::new(reference_counter);

    stack
        .push(StackItem::from_int(1))
        .expect("push should succeed");
    stack
        .push(StackItem::from_int(2))
        .expect("push should succeed");
    stack
        .push(StackItem::from_int(3))
        .expect("push should succeed");

    assert_eq!(stack.len(), 3);

    let item = stack.pop().expect("pop should succeed");
    assert_eq!(
        item.as_int().expect("as_int should succeed"),
        num_bigint::BigInt::from(3)
    );
    assert_eq!(stack.len(), 2);
}

#[test]
fn test_peek() {
    let reference_counter = ReferenceCounter::new();
    let mut stack = EvaluationStack::new(reference_counter);

    stack
        .push(StackItem::from_int(1))
        .expect("push should succeed");
    stack
        .push(StackItem::from_int(2))
        .expect("push should succeed");
    stack
        .push(StackItem::from_int(3))
        .expect("push should succeed");

    let item0 = stack.peek(0).expect("peek should succeed");
    let item1 = stack.peek(1).expect("peek should succeed");
    let item2 = stack.peek(2).expect("peek should succeed");

    assert_eq!(
        item0.as_int().expect("as_int should succeed"),
        num_bigint::BigInt::from(3)
    );
    assert_eq!(
        item1.as_int().expect("as_int should succeed"),
        num_bigint::BigInt::from(2)
    );
    assert_eq!(
        item2.as_int().expect("as_int should succeed"),
        num_bigint::BigInt::from(1)
    );
    assert_eq!(stack.len(), 3);
}

#[test]
fn test_insert_remove() -> Result<(), String> {
    let reference_counter = ReferenceCounter::new();
    let mut stack = EvaluationStack::new(reference_counter);

    stack
        .push(StackItem::from_int(1))
        .expect("push should succeed");
    stack
        .push(StackItem::from_int(3))
        .expect("push should succeed");
    stack
        .insert(1, StackItem::from_int(2))
        .expect("insert should succeed");

    assert_eq!(
        stack
            .peek(2)
            .expect("intermediate value should exist")
            .as_int()
            .expect("as_int should succeed"),
        num_bigint::BigInt::from(1)
    );
    assert_eq!(
        stack
            .peek(1)
            .expect("intermediate value should exist")
            .as_int()
            .expect("as_int should succeed"),
        num_bigint::BigInt::from(2)
    );
    assert_eq!(
        stack
            .peek(0)
            .expect("intermediate value should exist")
            .as_int()
            .expect("as_int should succeed"),
        num_bigint::BigInt::from(3)
    );

    let item = stack.remove(1).map_err(|err| err.to_string())?;
    assert_eq!(
        item.as_int().map_err(|err| err.to_string())?,
        num_bigint::BigInt::from(2)
    );
    assert_eq!(
        stack
            .peek(1)
            .expect("intermediate value should exist")
            .as_int()
            .map_err(|err| err.to_string())?,
        num_bigint::BigInt::from(1)
    );
    assert_eq!(
        stack
            .peek(0)
            .expect("intermediate value should exist")
            .as_int()
            .map_err(|err| err.to_string())?,
        num_bigint::BigInt::from(3)
    );
    Ok(())
}

#[test]
fn test_swap() {
    let reference_counter = ReferenceCounter::new();
    let mut stack = EvaluationStack::new(reference_counter);

    stack
        .push(StackItem::from_int(1))
        .expect("push should succeed");
    stack
        .push(StackItem::from_int(2))
        .expect("push should succeed");
    stack
        .push(StackItem::from_int(3))
        .expect("push should succeed");

    stack.swap(0, 2).expect("swap should succeed");

    assert_eq!(
        stack
            .peek(0)
            .expect("intermediate value should exist")
            .as_int()
            .expect("as_int should succeed"),
        num_bigint::BigInt::from(1)
    );
    assert_eq!(
        stack
            .peek(1)
            .expect("intermediate value should exist")
            .as_int()
            .expect("as_int should succeed"),
        num_bigint::BigInt::from(2)
    );
    assert_eq!(
        stack
            .peek(2)
            .expect("intermediate value should exist")
            .as_int()
            .expect("as_int should succeed"),
        num_bigint::BigInt::from(3)
    );
}

#[test]
fn test_reverse() {
    let reference_counter = ReferenceCounter::new();
    let mut stack = EvaluationStack::new(reference_counter);

    for value in 1..=5 {
        stack
            .push(StackItem::from_int(value))
            .expect("push should succeed");
    }

    stack.reverse(3).expect("reverse should succeed");

    assert_eq!(
        stack
            .peek(0)
            .expect("intermediate value should exist")
            .as_int()
            .expect("as_int should succeed"),
        num_bigint::BigInt::from(3)
    );
    assert_eq!(
        stack
            .peek(1)
            .expect("intermediate value should exist")
            .as_int()
            .expect("as_int should succeed"),
        num_bigint::BigInt::from(4)
    );
    assert_eq!(
        stack.peek(2).unwrap().as_int().unwrap(),
        num_bigint::BigInt::from(5)
    );
    assert_eq!(
        stack.peek(3).unwrap().as_int().unwrap(),
        num_bigint::BigInt::from(2)
    );
    assert_eq!(
        stack.peek(4).unwrap().as_int().unwrap(),
        num_bigint::BigInt::from(1)
    );

    stack.reverse(5).unwrap();

    assert_eq!(
        stack.peek(0).unwrap().as_int().unwrap(),
        num_bigint::BigInt::from(1)
    );
    assert_eq!(
        stack.peek(1).unwrap().as_int().expect("Operation failed"),
        num_bigint::BigInt::from(2)
    );
    assert_eq!(
        stack.peek(2).unwrap().as_int().expect("Operation failed"),
        num_bigint::BigInt::from(5)
    );
    assert_eq!(
        stack.peek(3).unwrap().as_int().expect("Operation failed"),
        num_bigint::BigInt::from(4)
    );
    assert_eq!(
        stack.peek(4).unwrap().as_int().expect("Operation failed"),
        num_bigint::BigInt::from(3)
    );

    stack.reverse(0).expect("Operation failed");
    assert_eq!(
        stack.peek(0).unwrap().as_int().expect("Operation failed"),
        num_bigint::BigInt::from(1)
    );
    assert_eq!(
        stack.peek(1).unwrap().as_int().expect("Operation failed"),
        num_bigint::BigInt::from(2)
    );

    stack.reverse(1).expect("Operation failed");
    assert_eq!(
        stack.peek(0).unwrap().as_int().expect("Operation failed"),
        num_bigint::BigInt::from(1)
    );

    assert!(stack.reverse(10).is_err());
}

#[test]
fn test_clear() {
    let reference_counter = ReferenceCounter::new();
    let mut stack = EvaluationStack::new(reference_counter);

    stack
        .push(StackItem::from_int(1))
        .expect("push should succeed");
    stack
        .push(StackItem::from_int(2))
        .expect("push should succeed");
    stack
        .push(StackItem::from_int(3))
        .expect("push should succeed");

    stack.clear();

    assert_eq!(stack.len(), 0);
    assert!(stack.is_empty());
}

#[test]
fn test_copy_to() {
    let reference_counter1 = ReferenceCounter::new();
    let reference_counter2 = ReferenceCounter::new();
    let mut stack1 = EvaluationStack::new(reference_counter1);
    let mut stack2 = EvaluationStack::new(reference_counter2);

    stack1
        .push(StackItem::from_int(1))
        .expect("push should succeed");
    stack1
        .push(StackItem::from_int(2))
        .expect("push should succeed");
    stack1
        .push(StackItem::from_int(3))
        .expect("push should succeed");

    stack1
        .copy_to(&mut stack2, None)
        .expect("copy_to should succeed");

    assert_eq!(stack1.len(), 3);
    assert_eq!(stack2.len(), 3);
    assert_eq!(
        stack1.peek(0).unwrap().as_int().unwrap(),
        num_bigint::BigInt::from(3)
    );
    assert_eq!(
        stack2.peek(0).unwrap().as_int().unwrap(),
        num_bigint::BigInt::from(3)
    );
}
