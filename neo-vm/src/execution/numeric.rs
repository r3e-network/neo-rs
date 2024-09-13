// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::math::I256;
use crate::*;

pub(crate) fn exec_sign(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let Some(item) = cx.stack.pop() else {
        return Err(ExecError::StackOutOfBound(op.ip, op.code, cx.stack.len()));
    };

    let StackItem::Integer(item) = *item else {
        return Err(ExecError::InvalidCast(op.ip, op.code, item.item_type()));
    };

    // TODO: ok or not
    let _ok = cx.stack.push(Rc::new(StackItem::Integer(I256::from(item.sign()))));
    Ok(())
}