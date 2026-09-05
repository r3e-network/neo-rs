//! Shared NeoVM syscall helpers.

use sha2::{Digest, Sha256};

#[derive(Clone, Copy)]
#[cfg(test)]
struct KnownSyscall {
    name: &'static str,
    hash: u32,
    arg_count: usize,
}

macro_rules! define_syscalls {
    ($(
        $name:literal => $hash:expr, args = $arg_count:expr;
    )+) => {
        #[cfg(test)]
        const KNOWN_SYSCALLS: &[KnownSyscall] = &[
            $(
                KnownSyscall {
                    name: $name,
                    hash: $hash,
                    arg_count: $arg_count,
                },
            )+
        ];

        #[inline]
        fn known_interop_hash(name: &str) -> Option<u32> {
            match name {
                $(
                    $name => Some($hash),
                )+
                _ => None,
            }
        }

        /// Returns the number of stack arguments consumed by a known NeoVM syscall.
        ///
        /// Unknown or count-dependent syscalls return `usize::MAX`, which tells callers
        /// to pass the full stack through the host boundary.
        #[must_use]
        #[inline]
        pub fn syscall_arg_count(api: u32) -> usize {
            match api {
                $(
                    $hash => $arg_count,
                )+
                _ => usize::MAX,
            }
        }
    };
}

define_syscalls! {
    // System.Contract
    "System.Contract.Call" => 0x525b_7d62, args = 4;
    "System.Contract.CallNative" => 0x677b_f71a, args = usize::MAX;
    "System.Contract.Create" => 0x852c_35ce, args = 2;
    "System.Contract.Update" => 0x1d33_c631, args = 2;
    "System.Contract.NativeOnPersist" => 0x93bc_db2e, args = 0;
    "System.Contract.NativePostPersist" => 0x165d_a144, args = 0;
    "System.Contract.GetCallFlags" => 0x813a_da95, args = 0;
    "System.Contract.CreateStandardAccount" => 0x0287_99cf, args = 1;
    "System.Contract.CreateMultisigAccount" => 0x09e9_336a, args = 2;

    // System.Runtime
    "System.Runtime.CheckWitness" => 0x8cec_27f8, args = 1;
    "System.Runtime.Notify" => 0x616f_0195, args = 2;
    "System.Runtime.Log" => 0x9647_e7cf, args = 1;
    "System.Runtime.GetNotifications" => 0xf135_4327, args = 1;
    "System.Runtime.BurnGas" => 0xbc8c_5ac3, args = 1;
    "System.Runtime.LoadScript" => 0x8f80_0cb3, args = 3;
    "System.Runtime.Platform" => 0xf6fc_79b2, args = 0;
    "System.Runtime.GetTrigger" => 0xa038_7de9, args = 0;
    "System.Runtime.GetTime" => 0x0388_c3b7, args = 0;
    "System.Runtime.GetScriptContainer" => 0x3008_512d, args = 0;
    "System.Runtime.GetExecutingScriptHash" => 0x74a8_fedb, args = 0;
    "System.Runtime.GetCallingScriptHash" => 0x3c6e_5339, args = 0;
    "System.Runtime.GetEntryScriptHash" => 0x38e2_b4f9, args = 0;
    "System.Runtime.GetInvocationCounter" => 0x4311_2784, args = 0;
    "System.Runtime.GasLeft" => 0xced8_8814, args = 0;
    "System.Runtime.GetAddressVersion" => 0xdc92_494c, args = 0;
    "System.Runtime.CurrentSigners" => 0x8b18_f1ac, args = 0;
    "System.Runtime.GetNetwork" => 0xe0a0_fbc5, args = 0;
    "System.Runtime.GetRandom" => 0x28a9_de6b, args = 0;

    // System.Storage
    "System.Storage.GetContext" => 0xce67_f69b, args = 0;
    "System.Storage.GetReadOnlyContext" => 0xe26b_b4f6, args = 0;
    "System.Storage.AsReadOnly" => 0xe9bf_4c76, args = 1;
    "System.Storage.Local.Get" => 0xe85e_8dd5, args = 1;
    "System.Storage.Local.Put" => 0x0ae3_0c39, args = 2;
    "System.Storage.Local.Delete" => 0x94f5_5475, args = 1;
    "System.Storage.Local.Find" => 0xf352_7607, args = 2;
    "System.Storage.Get" => 0x31e8_5d92, args = 2;
    "System.Storage.Find" => 0x9ab8_30df, args = 3;
    "System.Storage.Put" => 0x8418_3fe6, args = 3;
    "System.Storage.Delete" => 0xedc5_582f, args = 2;

    // System.Crypto
    "System.Crypto.CheckSig" => 0x27b3_e756, args = 2;
    "System.Crypto.CheckMultisig" => 0x3adc_d09e, args = usize::MAX;

    // System.Iterator
    "System.Iterator.Next" => 0x9ced_089c, args = 1;
    "System.Iterator.Value" => 0x1dbf_54f3, args = 1;
}

/// Returns the first four bytes of SHA-256(name) as a little-endian syscall id.
#[must_use]
#[inline]
pub fn interop_hash(name: &str) -> u32 {
    if let Some(hash) = known_interop_hash(name) {
        return hash;
    }

    let digest = Sha256::digest(name.as_bytes());
    u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::{KNOWN_SYSCALLS, interop_hash, syscall_arg_count};

    fn canonical_hash(name: &str) -> u32 {
        let digest = Sha256::digest(name.as_bytes());
        u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]])
    }

    #[test]
    fn known_syscall_hashes_match_sha256_prefix() {
        for syscall in KNOWN_SYSCALLS {
            assert_eq!(
                syscall.hash,
                canonical_hash(syscall.name),
                "metadata hash mismatch for {}",
                syscall.name
            );
            assert_eq!(
                interop_hash(syscall.name),
                syscall.hash,
                "hash mismatch for {}",
                syscall.name
            );
        }
    }

    #[test]
    fn known_syscall_argument_counts_come_from_metadata() {
        for syscall in KNOWN_SYSCALLS {
            assert_eq!(
                syscall_arg_count(interop_hash(syscall.name)),
                syscall.arg_count,
                "argument count mismatch for {}",
                syscall.name
            );
        }
    }

    #[test]
    fn unknown_syscall_hashes_still_use_sha256_prefix() {
        let name = "System.Test.Unknown";
        assert_eq!(interop_hash(name), canonical_hash(name));
    }

    #[test]
    fn known_syscall_argument_counts_match_hashes() {
        assert_eq!(syscall_arg_count(interop_hash("System.Contract.Call")), 4);
        assert_eq!(
            syscall_arg_count(interop_hash("System.Contract.CallNative")),
            usize::MAX
        );
        assert_eq!(
            syscall_arg_count(interop_hash("System.Runtime.Platform")),
            0
        );
    }
}
