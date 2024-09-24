// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::*;

pub(crate) fn exec_invert(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let item = pop!(cx, op)
        .as_int()
        .map_err(|err| ExecError::InvalidCast(op.ip, op.code, err.item_type()))?;
    push_checked!(cx, op, StackItem::with_integer(!item))
}

pub(crate) fn exec_and(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let first = pop!(cx, op)
        .as_int()
        .map_err(|err| ExecError::InvalidCast(op.ip, op.code, err.item_type()))?;
    let second = pop!(cx, op)
        .as_int()
        .map_err(|err| ExecError::InvalidCast(op.ip, op.code, err.item_type()))?;

    push_checked!(cx, op, StackItem::with_integer(first & second))
}

pub(crate) fn exec_or(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let first = pop!(cx, op)
        .as_int()
        .map_err(|err| ExecError::InvalidCast(op.ip, op.code, err.item_type()))?;
    let second = pop!(cx, op)
        .as_int()
        .map_err(|err| ExecError::InvalidCast(op.ip, op.code, err.item_type()))?;

    push_checked!(cx, op, StackItem::with_integer(first | second))
}

pub(crate) fn exec_xor(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let first = pop!(cx, op)
        .as_int()
        .map_err(|err| ExecError::InvalidCast(op.ip, op.code, err.item_type()))?;
    let second = pop!(cx, op)
        .as_int()
        .map_err(|err| ExecError::InvalidCast(op.ip, op.code, err.item_type()))?;

    push_checked!(cx, op, StackItem::with_integer(first ^ second))
}

// Equal
pub(crate) fn exec_equal(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let first = pop!(cx, op);
    let second = pop!(cx, op);

    let eq = first
        .checked_eq(&second)
        .map_err(|_err| ExecError::ExceedExecutionLimits("compare limits"))?;
    push_checked!(cx, op, StackItem::with_boolean(eq))
}

// NotEqual
pub(crate) fn exec_not_equal(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let first = pop!(cx, op);
    let second = pop!(cx, op);

    let eq = first
        .checked_eq(&second)
        .map_err(|_err| ExecError::ExceedExecutionLimits("compare limits"))?;
    push_checked!(cx, op, StackItem::with_boolean(!eq))
}

#[cfg(test)]
mod test {
    use neo_base::math::I256;

    use super::*;

    #[test]
    fn test_exec_invert() {
        let mut cx =
            ExecContext::new(ExecStack::new(1024, References::new()), Rc::new(Program::nop()));

        let op = Op { ip: 1, code: OpCode::Invert, operand: Default::default() };
        let _ = exec_invert(&mut cx, &op).expect_err("empty stack should be failed");

        cx.stack.push(StackItem::with_integer(1.into()));

        let _ = exec_invert(&mut cx, &op).expect("invent -1 should be ok");
        assert_eq!(cx.stack.len(), 1);

        let item = cx.stack.top().expect("`top()` should be exists");

        if let StackItem::Integer(value) = item {
            assert_eq!(*value, I256::from(!1i32));
        } else {
            assert!(false);
        }
    }
}
