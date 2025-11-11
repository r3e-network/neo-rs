mod arithmetic;
mod control;
mod push;
mod stack;
mod syscall;

pub(super) use arithmetic::apply_arithmetic;
pub(super) use control::apply_control;
pub(super) use push::apply_push;
pub(super) use stack::apply_stack;
pub(super) use syscall::apply_syscall;
