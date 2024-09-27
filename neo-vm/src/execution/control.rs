// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::*;

pub(crate) fn exec_jmp(cx: &mut ExecutionContext, op: &Op) -> Result<(), ExecError> {
    let target = op.ip + op.operand.first as i8 as u32;
    if !cx.change_pc(target) {
        return Err(ExecError::InvalidJumpTarget(op.ip, op.code, target));
    }
    Ok(())
}
