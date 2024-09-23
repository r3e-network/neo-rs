/// Package contract provides functions to work with contracts.

use crate::interop;
use crate::interop::neogointernal;

/// CallFlag specifies valid call flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallFlag(u8);

/// Using `smartcontract` package from compiled contract requires moderate
/// compiler refactoring, thus all flags are mirrored here.
impl CallFlag {
    pub const READ_STATES: CallFlag = CallFlag(1 << 0);
    pub const WRITE_STATES: CallFlag = CallFlag(1 << 1);
    pub const ALLOW_CALL: CallFlag = CallFlag(1 << 2);
    pub const ALLOW_NOTIFY: CallFlag = CallFlag(1 << 3);
    pub const STATES: CallFlag = CallFlag(Self::READ_STATES.0 | Self::WRITE_STATES.0);
    pub const READ_ONLY: CallFlag = CallFlag(Self::READ_STATES.0 | Self::ALLOW_CALL.0);
    pub const ALL: CallFlag = CallFlag(Self::STATES.0 | Self::ALLOW_CALL.0 | Self::ALLOW_NOTIFY.0);
    pub const NONE_FLAG: CallFlag = CallFlag(0);
}

/// CreateMultisigAccount calculates a script hash of an m out of n multisignature
/// script using the given m and a set of public keys bytes. This function uses
/// `System.Contract.CreateMultisigAccount` syscall.
pub fn create_multisig_account(m: i32, pubs: Vec<interop::PublicKey>) -> Vec<u8> {
    neogointernal::syscall2("System.Contract.CreateMultisigAccount", m, pubs)
}

/// CreateStandardAccount calculates a script hash of the given public key.
/// This function uses `System.Contract.CreateStandardAccount` syscall.
pub fn create_standard_account(pub_key: interop::PublicKey) -> Vec<u8> {
    neogointernal::syscall1("System.Contract.CreateStandardAccount", pub_key)
}

/// GetCallFlags returns the calling flags which execution context was created with.
/// This function uses `System.Contract.GetCallFlags` syscall.
pub fn get_call_flags() -> CallFlag {
    neogointernal::syscall0("System.Contract.GetCallFlags")
}

/// Call executes the previously deployed blockchain contract with the specified hash
/// (20 bytes in BE form) using the provided arguments and call flags.
/// It returns whatever this contract returns. This function uses
/// `System.Contract.Call` syscall.
pub fn call(script_hash: interop::Hash160, method: &str, f: CallFlag, args: Vec<impl Any>) -> Box<dyn Any> {
    neogointernal::syscall4("System.Contract.Call", script_hash, method, f, args)
}
