use crate::interop;
use crate::interop::contract;
use crate::interop::native::management;

/// CallWithVersion is a utility function that executes the previously deployed
/// blockchain contract with the specified version (update counter) and hash
/// (20 bytes in BE form) using the provided arguments and call flags. It fails
/// if the contract has version mismatch. It returns whatever this contract
/// returns. This function uses `System.Contract.Call` syscall.
pub fn call_with_version(script_hash: interop::Hash160, version: i32, method: &str, f: contract::CallFlag, args: Vec<Box<dyn std::any::Any>>) -> Box<dyn std::any::Any> {
    let cs = management::get_contract(script_hash);
    if cs.is_none() {
        panic!("unknown contract");
    }
    let cs = cs.unwrap();
    if cs.update_counter != version {
        panic!("contract version mismatch");
    }
    contract::call(script_hash, method, f, args)
}
