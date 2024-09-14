pub mod instruction;
pub mod op_code;

pub mod script;

pub mod evaluation_stack;

pub mod execution_context;

pub mod slot;

pub mod execution_engine;
pub mod vm_error;
pub mod vm_state;
pub mod execution_engine_limits;


pub use instruction::*;
pub use op_code::*;
pub use script::*;
pub use evaluation_stack::*;
pub use execution_context::*;
pub use slot::*;
pub use execution_engine::*;
pub use vm_error::*;
pub use vm_state::*;
pub use execution_engine_limits::*;


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
