// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::bytes::ToArray;
use neo_base::math::I256;

use crate::*;

#[inline]
pub(crate) fn exec_push_int(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    push_checked!(cx, op, StackItem::with_integer(op.operand.first.into()))
}

pub(crate) fn exec_push_int128(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let data = op.operand.data.as_slice();
    if data.len() != 16 {
        return Err(ExecError::InvalidExecution(op.ip, op.code, "data.len() must be 16"));
    }

    let v = i128::from_le_bytes(data.to_array());
    push_checked!(cx, op, StackItem::with_integer(v.into()))
}

pub(crate) fn exec_push_int256(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let data = op.operand.data.as_slice();
    if data.len() != 32 {
        return Err(ExecError::InvalidExecution(op.ip, op.code, "data.len() must be 32"));
    }

    let v = I256::from_le_bytes(data.to_array());
    push_checked!(cx, op, StackItem::with_integer(v))
}
