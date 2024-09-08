#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ExceptionHandlingState {
	/// Indicates that the `try` block is being executed.
	Try,

	/// Indicates that the `catch` block is being executed.
	Catch,

	/// Indicates that the `finally` block is being executed.
	Finally,
}
