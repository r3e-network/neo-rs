// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::math::I256;

use crate::{StackItem::Integer, *};

pub(crate) fn exec_pack_map(cx: &mut ExecutionContext, op: &Op) -> Result<(), ExecError> {
    let size = pop_as_typed!(cx, op, Integer);
    if size.is_negative() || size > I256::I32_MAX {
        // TODO: better error
        return Err(ExecError::StackOutOfBound(op.ip, op.code, cx.stack.len()));
    }

    let n = size.as_i128() as usize;
    if n * 2 > cx.stack.len() {
        return Err(ExecError::StackOutOfBound(op.ip, op.code, cx.stack.len()));
    }

    let map = Map::with_capacity(n);
    let mut items = map.items_mut();
    for _ in 0..n {
        let key = pop!(cx, op);
        if !key.primitive_type() {
            return Err(ExecError::InvalidExecution(op.ip, op.code, "Key must be primitive type"));
        }

        let value = pop!(cx, op);
        items.insert(key, value);
    }
    drop(items);

    push_checked!(cx, op, StackItem::Map(map))
}
