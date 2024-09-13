// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use crate::*;


pub(crate) fn exec_is_null(cx: &mut ExecContext, op: &Op) -> Result<(), ExecError> {
    let Some(item) = cx.stack.pop() else {
        return Err(ExecError::StackOutOfBound(op.ip, op.code, cx.stack.len()));
    };

    let is_null = matches!(item.as_ref(), StackItem::Null);
    // TODO: ok or not
    let _ok = cx.stack.push(Rc::new(StackItem::Boolean(is_null)));
    Ok(())
}


#[cfg(test)]
mod test {
    //
}