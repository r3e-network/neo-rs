// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::{pop_as_typed, push_checked, ExecError, Op};
use crate::stack_item::StackItem;
use crate::stack_item::StackItem::Integer;
use crate::vm::ExecutionContext;

pub(crate) fn exec_sign(cx: &mut ExecutionContext, op: &Op) -> Result<(), ExecError> {
    let item = pop_as_typed!(cx, op, Integer);
    push_checked!(cx, op, StackItem::with_integer(item.sign().into()))
}
