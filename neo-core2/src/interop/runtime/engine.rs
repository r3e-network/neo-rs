use crate::interop;
use crate::interop::native::ledger;
use crate::interop::neogointernal;

// GetScriptContainer returns the transaction that initially triggered current
// execution context. It never changes in a single execution, no matter how deep
// this execution goes. This function uses
// `System.Runtime.GetScriptContainer` syscall.
pub fn get_script_container() -> &'static ledger::Transaction {
    neogointernal::syscall0("System.Runtime.GetScriptContainer").downcast_ref::<ledger::Transaction>().unwrap()
}

// GetExecutingScriptHash returns script hash (160 bit in BE form represented
// as 20-byte slice) of the contract that is currently being executed. Any
// AppCall can change the value returned by this function if it calls a
// different contract. This function uses
// `System.Runtime.GetExecutingScriptHash` syscall.
pub fn get_executing_script_hash() -> interop::Hash160 {
    neogointernal::syscall0("System.Runtime.GetExecutingScriptHash").downcast_ref::<interop::Hash160>().unwrap().clone()
}

// GetCallingScriptHash returns script hash (160 bit in BE form represented
// as 20-byte slice) of the contract that started the execution of the currently
// running context (caller of current contract or function), so it's one level
// above the GetExecutingScriptHash in the call stack. It uses
// `System.Runtime.GetCallingScriptHash` syscall.
pub fn get_calling_script_hash() -> interop::Hash160 {
    neogointernal::syscall0("System.Runtime.GetCallingScriptHash").downcast_ref::<interop::Hash160>().unwrap().clone()
}

// GetEntryScriptHash returns script hash (160 bit in BE form represented
// as 20-byte slice) of the contract that initially started current execution
// (this is a script that is contained in a transaction returned by
// GetScriptContainer) execution from the start. This function uses
// `System.Runtime.GetEntryScriptHash` syscall.
pub fn get_entry_script_hash() -> interop::Hash160 {
    neogointernal::syscall0("System.Runtime.GetEntryScriptHash").downcast_ref::<interop::Hash160>().unwrap().clone()
}
