// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::*;

pub(crate) fn exec_is_null(cx: &mut ExecutionContext, op: &Op) -> Result<(), ExecError> {
    let item = pop!(cx, op);
    push_checked!(cx, op, StackItem::with_boolean(item.is_null()))
}

#[cfg(test)]
mod test {
    //
}
