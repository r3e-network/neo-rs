/*
Package util contains some special useful functions that are provided by compiler and VM.
*/

use crate::interop::neogointernal;

// Abort terminates current execution, unlike exception throwing with panic() it
// can't be recovered from.
pub fn abort() {
    neogointernal::opcode0_no_return("ABORT");
}

// AbortMsg terminates current execution with the specified message. Unlike
// exception throwing with panic() it can't be recovered from.
pub fn abort_msg(msg: &str) {
    neogointernal::opcode1_no_return("ABORTMSG", msg);
}

// Assert terminates current execution if the condition specified is false. Unlike
// exception throwing with panic() it can't be recovered from.
pub fn assert(ok: bool) {
    neogointernal::opcode1_no_return("ASSERT", ok);
}

// AssertMsg terminates current execution with the specified message if the
// condition specified is false. Unlike exception throwing with panic() it can't
// be recovered from.
pub fn assert_msg(ok: bool, msg: &str) {
    neogointernal::opcode2_no_return("ASSERTMSG", ok, msg);
}

// Equals compares a with b and will return true when a and b are equal. It's
// implemented as an EQUAL VM opcode, so the rules of comparison are those
// of EQUAL.
pub fn equals(a: &dyn std::any::Any, b: &dyn std::any::Any) -> bool {
    neogointernal::opcode2("EQUAL", a, b).downcast_ref::<bool>().copied().unwrap_or(false)
}

// Remove removes element with index i from slice.
// This is done in place and slice must have type other than `[]byte`.
pub fn remove(slice: &mut dyn std::any::Any, i: i32) {
    neogointernal::opcode2_no_return("REMOVE", slice, i);
}
