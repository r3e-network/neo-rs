// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::*;

pub(crate) fn exec_depth(cx: &mut ExecutionContext, op: &Op) -> Result<(), ExecError> {
    push_checked!(cx, op, StackItem::with_integer(cx.stack.len().into()))
}

pub(crate) fn exec_drop(cx: &mut ExecutionContext, op: &Op) -> Result<(), ExecError> {
    let _dropped = pop!(cx, op);
    Ok(())
}
