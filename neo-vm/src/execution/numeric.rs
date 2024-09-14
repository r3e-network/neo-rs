// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::math::I256;
use crate::*;

pub(crate) fn exec_sign(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let item = pop_as_type!(cx, op, StackItem::Integer);

    // TODO: ok or not
    let _ok = cx.stack.push(Rc::new(StackItem::Integer(I256::from(item.sign()))));
    Ok(())
}