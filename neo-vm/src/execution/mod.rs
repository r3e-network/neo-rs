// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use crate::{ExecContext, ExecError, Op};

pub use executor::*;
pub(crate) use self::{bitwise::*, compound::*, control::*};
pub(crate) use self::{numeric::*, slot::*, stack::*, types::*};


pub mod bitwise;
pub mod compound;
pub mod control;
pub mod executor;
pub mod numeric;
pub mod slot;
pub mod stack;
pub mod types;

#[cfg(test)]
mod executor_test;


#[macro_export]
macro_rules! pop_as_type {
    ($cx:ident, $op:ident, $item_type:path) => {{
        let Some(item) = $cx.stack.pop() else {
            return Err(ExecError::StackOutOfBound($op.ip, $op.code, $cx.stack.len()));
        };

        let $item_type(item) = *item else {
            return Err(ExecError::InvalidCast($op.ip, $op.code, item.item_type()));
        };
        item
    }};
}

#[cfg(test)]
mod test {
    //
}