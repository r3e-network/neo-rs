// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::{StackItem::Integer, *};

pub(crate) fn exec_sign(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let item = pop_as_typed!(cx, op, Integer);
    push_checked!(cx, op, StackItem::with_integer(item.sign().into()))
}
