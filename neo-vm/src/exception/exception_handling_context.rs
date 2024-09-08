use crate::exception::exception_handling_state::ExceptionHandlingState;

#[derive(Copy, Clone)]
pub struct ExceptionHandlingContext {
	pub(crate) catch_pointer: i32,
	pub(crate) finally_pointer: i32,
	pub(crate) end_pointer: i32,
	pub(crate) state: ExceptionHandlingState,
}

impl ExceptionHandlingContext {
	pub fn new(catch_pointer: i32, finally_pointer: i32) -> Self {
		Self { catch_pointer, finally_pointer, end_pointer: -1, state: ExceptionHandlingState::Try }
	}

	/// The position of the `catch` block.
	pub fn catch_pointer(&self) -> i32 {
		self.catch_pointer
	}

	/// The position of the `finally` block.
	pub fn finally_pointer(&self) -> i32 {
		self.finally_pointer
	}

	/// The end position of the `try`-`catch`-`finally` block.
	pub fn end_pointer(&self) -> i32 {
		self.end_pointer
	}

	/// Indicates whether the `catch` block is included in the context.
	pub fn has_catch(&self) -> bool {
		self.catch_pointer >= 0
	}

	/// Indicates whether the `finally` block is included in the context.
	pub fn has_finally(&self) -> bool {
		self.finally_pointer >= 0
	}

	/// Indicates the state of the context.
	pub fn state(&self) -> ExceptionHandlingState {
		self.state
	}

	/// Sets the end pointer value
	pub fn set_end_pointer(&mut self, end_pointer: i32) {
		self.end_pointer = end_pointer;
	}

	/// Sets the state
	pub fn set_state(&mut self, state: ExceptionHandlingState) {
		self.state = state;
	}
}
