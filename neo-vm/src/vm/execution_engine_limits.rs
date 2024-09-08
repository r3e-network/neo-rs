use std::num::NonZeroU32;

/// Represents the restrictions on the vm.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct ExecutionEngineLimits {
	/// The maximum number of bits that `OpCode::SHL` and `OpCode::SHR` can shift.
	pub max_shift: usize,

	/// The maximum number of items that can be contained in the vm's evaluation stacks and slots.
	pub max_stack_size: usize,

	/// The maximum size of an item in the vm.
	pub max_item_size: usize,

	/// The largest comparable size. If a `ByteString` or `Struct` exceeds this size, comparison operations on it cannot be performed in the vm.
	pub max_comparable_size: usize,

	/// The maximum number of frames in the invocation stack of the vm.
	pub max_invocation_stack_size: usize,

	/// The maximum nesting depth of `try` blocks.
	pub max_try_nesting_depth: usize,

	/// Allow catching the ExecutionEngine Exceptions
	pub catch_engine_exceptions: bool,
}

impl Default for ExecutionEngineLimits {
	fn default() -> Self {
		Self {
			max_shift: 256,
			max_stack_size: 2 * 1024,
			max_item_size: 1024 * 1024,
			max_comparable_size: 65536,
			max_invocation_stack_size: 1024,
			max_try_nesting_depth: 16,
			catch_engine_exceptions: true,
		}
	}
}

impl ExecutionEngineLimits {
	/// Assert that the size of the item meets the limit.
	#[inline]
	pub fn assert_max_item_size(&self, size: u32) {
		if size == 0 || size > self.max_item_size as u32 {
			panic!("MaxItemSize exceeded: {size}");
		}
	}

	/// Assert that the number of bits shifted meets the limit.
	#[inline]
	pub fn assert_shift(&self, shift: i32) {
		if shift > self.max_shift as i32 || shift < 0 {
			panic!("Invalid shift value: {shift}");
		}
	}
}
