/// Module crypto provides an interface to cryptographic syscalls.

use crate::interop;
use crate::interop::neogointernal;

/// CheckMultisig checks that the script container (transaction) is signed by multiple
/// ECDSA keys at once. It uses `System.Crypto.CheckMultisig` syscall.
pub fn check_multisig(pubs: Vec<interop::PublicKey>, sigs: Vec<interop::Signature>) -> bool {
    neogointernal::syscall2("System.Crypto.CheckMultisig", pubs, sigs)
}

/// CheckSig checks that sig is a correct signature of the script container
/// (transaction) for the given pub (serialized public key). It uses
/// `System.Crypto.CheckSig` syscall.
pub fn check_sig(pub: interop::PublicKey, sig: interop::Signature) -> bool {
    neogointernal::syscall2("System.Crypto.CheckSig", pub, sig)
}
