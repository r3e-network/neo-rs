// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::{StackItem::Integer, *};

pub(crate) fn exec_invert(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let item = pop_as_typed!(cx, op, Integer);
    push_checked!(cx, op, StackItem::with_integer(!item))
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

        if let Integer(value) = item {
            assert_eq!(*value, I256::from(!1i32));
        } else {
            assert!(false);
        }
    }
}
