pub mod instruction;
pub mod op_code;

pub mod script;

pub mod evaluation_stack;

pub mod execution_context;

pub mod slot;

pub mod execution_engine;
pub mod execution_engine_limits;
pub mod vm_state;

pub use evaluation_stack::*;
pub use execution_context::*;
pub use execution_engine::*;
pub use execution_engine_limits::*;
pub use instruction::*;
pub use op_code::*;
pub use script::*;
pub use slot::*;
pub use vm_state::*;

pub use crate::vm_error::*;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
