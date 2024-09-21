// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::*;

pub(crate) fn exec_init_sslot(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    if cx.statics.is_some() {
        return Err(ExecError::InvalidExecution(op.ip, op.code, "it can only be executed once"));
    }

    let n = op.operand.first;
    if n <= 0 {
        return Err(ExecError::InvalidOperand(op.ip, op.code, n));
    }

    cx.statics = Some(Slots::new(n as usize));
    Ok(())
}

pub(crate) fn exec_init_slot(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let locals = op.operand.first;
    let arguments = op.operand.second;
    if cx.locals.is_some() || cx.arguments.is_some() {
        return Err(ExecError::InvalidExecution(op.ip, op.code, "it can only be executed once"));
    }

    if locals <= 0 && arguments <= 0 {
        return Err(ExecError::InvalidOperand(op.ip, op.code, locals));
    }

    if locals > 0 {
        cx.locals = Some(Slots::new(locals as usize));
    }

    if arguments > 0 {
        cx.arguments = Some(Slots::new(arguments as usize));
    }
    Ok(())
}

pub(crate) fn exec_load_static_n<const N: usize>(
    cx: &mut ExecContext,
    op: &Op,
) -> Result<(), ExecError> {
    load_static(cx, op, N)
}

pub(crate) fn exec_load_static(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    load_static(cx, op, op.operand.first as usize)
}

fn load_static(cx: &mut ExecContext, op: &Op, n: usize) -> Result<(), ExecError> {
    let Some(statics) = cx.statics.as_ref() else {
        return Err(ExecError::InvalidExecution(op.ip, op.code, "static slots not initialized"));
    };

    let Some(item) = statics.get(n) else {
        return Err(ExecError::IndexOutOfBound(op.ip, op.code, n, statics.len()));
    };

    push_checked!(cx, op, item.clone())
}
