// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use neo_base::math::I256;
use crate::*;
use crate::vm_types::stack_item::StackItem;

pub(crate) fn exec_depth(cx: &mut ExecContext, _op: &Op) -> Result<(), ExecError> {
    let depth = I256::from(cx.stack.len() as i128);
    let _ok = cx.stack.push(Rc::new(StackItem::Integer(depth)));
    Ok(())
}