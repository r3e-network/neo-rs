// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

pub use executor::*;
pub(crate) use {bitwise::*, compound::*, control::*, push::*};
pub(crate) use {numeric::*, slot::*, stack::*, types::*};

use crate::{ExecutionContext, ExecError, Op};

pub mod bitwise;
pub mod compound;
pub mod control;
pub mod executor;
pub mod numeric;
pub mod push;
pub mod slot;
pub mod stack;
pub mod types;

#[cfg(test)]
mod executor_test;

#[macro_export]
macro_rules! pop_as_typed {
    ($cx:ident, $op:ident, $item_type:path) => {{
        let Some(item) = $cx.stack.pop() else {
            return Err(ExecError::StackOutOfBound($op.ip, $op.code, $cx.stack.len()));
        };

        let $item_type(item) = item else {
            return Err(ExecError::InvalidCast($op.ip, $op.code, item.item_type()));
        };

        item
    }};
}

#[macro_export]
macro_rules! pop {
    ($cx:ident, $op:ident) => {{
        let Some(item) = $cx.stack.pop() else {
            return Err(ExecError::StackOutOfBound($op.ip, $op.code, $cx.stack.len()));
        };

        item
    }};
}

#[macro_export]
macro_rules! push_checked {
    ($cx:ident, $op:ident, $item:expr) => {{
        if $cx.stack.push($item) {
            Ok(())
        } else {
            Err(ExecError::StackOutOfBound($op.ip, $op.code, $cx.stack.len()))
        }
    }};
}

#[cfg(test)]
mod test {
    //
}
