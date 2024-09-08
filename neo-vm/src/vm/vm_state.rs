#[repr(u8)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum VMState {
	/// Indicates that the execution is in progress or has not yet begun.
	None = 0,

	/// Indicates that the execution has been completed successfully.
	Halt = 1,

	/// Indicates that the execution has ended, and an exception that cannot be caught is thrown.
	Fault = 2,

	/// Indicates that a breakpoint is currently being hit.
	Break = 4,
}
