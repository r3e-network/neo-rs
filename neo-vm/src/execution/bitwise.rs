// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use crate::*;


pub(crate) fn exec_invert(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let Some(item) = cx.stack.pop() else {
        return Err(ExecError::StackOutOfBound(op.ip, op.code, cx.stack.len()));
    };

    let StackItem::Integer(item) = *item else {
        return Err(ExecError::InvalidCast(op.ip, op.code, item.item_type()));
    };

    let _ok = cx.stack.push(Rc::new(StackItem::Integer(!item)));
    Ok(())
}


#[cfg(test)]
mod test {
    use neo_base::math::I256;
    use super::*;

    #[test]
    fn test_exec_invert() {
        let mut cx = ExecContext::new(
            ExecStack::new(1024, Rc::new(References::new())),
            Rc::new(Program::nop()),
        );
        let op = Op { ip: 1, code: OpCode::Invert, operand: Default::default() };

        let _ = exec_invert(&mut cx, &op)
            .expect_err("empty stack should be failed");

        cx.stack.push(Rc::new(StackItem::Integer(I256::from(1))));

        let _ = exec_invert(&mut cx, &op)
            .expect("invent -1 should be ok");
        assert_eq!(cx.stack.len(), 1);

        if let Some(StackItem::Integer(item)) = cx.stack.top().as_deref() {
            assert_eq!(*item, I256::from(!1));
        } else {
            assert!(false);
        }
    }
}