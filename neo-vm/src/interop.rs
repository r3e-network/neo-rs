// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use core::hash::{Hash, Hasher};

use neo_core::contract::CallFlags;
use strum::{Display, EnumIter, EnumString};

use crate::{InteropCall::*, RunPrice};

#[derive(Debug, Clone)]
pub struct Interop {
    // TODO
}

impl Hash for Interop {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) { state.write_u8(0xff); }
}

impl PartialEq<Self> for Interop {
    fn eq(&self, _other: &Self) -> bool { false } // TODO
}

impl Eq for Interop {}

#[derive(Copy, Clone, PartialEq, Eq, EnumString, Display, EnumIter)]
#[repr(u8)]
pub enum InteropCall {
    // === Contract ===

    // Price: 1 << 15, CallFlags: ReadStates | AllowCall, ParamCount: 4
    #[strum(serialize = "System.Contract.Call")]
    SystemContractCall,

    // Price: 0, ParamCount: 1
    #[strum(serialize = "System.Contract.CallNative")]
    SystemContractCallNative,

    // Price: 1 << 10
    #[strum(serialize = "System.Contract.GetCallFlags")]
    SystemContractGetCallFlags,

    // Price: 0, ParamCount: 1
    #[strum(serialize = "System.Contract.CreateStandardAccount")]
    SystemContractCreateStandardAccount,

    // Price: 0, ParamCount: 2
    #[strum(serialize = "System.Contract.CreateMultisigAccount")]
    SystemContractCreateMultiSigAccount,

    // Price: 0, CallFlags: States
    #[strum(serialize = "System.Contract.NativeOnPersist")]
    SystemContractNativeOnPersist,

    // Price: 0, CallFlags: States
    #[strum(serialize = "System.Contract.NativePostPersist")]
    SystemContractNativePostPersist,

    // === Crypto ===

    // Price: 1 << 15, ParamCount: 2
    #[strum(serialize = "System.Crypto.CheckSig")]
    SystemCryptoCheckSig,

    // Price: 0, ParamCount: 2
    #[strum(serialize = "System.Crypto.CheckMultisig")]
    SystemCryptoCheckMultiSig,

    // === Iterator ===

    // Price: 1 << 15, ParamCount: 1
    #[strum(serialize = "System.Iterator.Next")]
    SystemIteratorNext,

    // Price: 1 << 4, ParamCount: 1
    #[strum(serialize = "System.Iterator.Value")]
    SystemIteratorValue,

    // === Runtime ===

    // Price: 1 << 3
    #[strum(serialize = "System.Runtime.Platform")]
    SystemRuntimePlatform,

    // Price: 1 << 3
    #[strum(serialize = "System.Runtime.GetTrigger")]
    SystemRuntimeGetTrigger,

    // Price: 1 << 3, CallFlags: ReadStates
    #[strum(serialize = "System.Runtime.GetTime")]
    SystemRuntimeGetTime,

    // Price: 1 << 3
    #[strum(serialize = "System.Runtime.GetScriptContainer")]
    SystemRuntimeGetScriptContainer,

    // Price: 1 << 4
    #[strum(serialize = "System.Runtime.GetExecutingScriptHash")]
    SystemRuntimeGetExecutingScriptHash,

    // Price: 1 << 4
    #[strum(serialize = "System.Runtime.GetCallingScriptHash")]
    SystemRuntimeGetCallingScriptHash,

    // Price: 1 << 4
    #[strum(serialize = "System.Runtime.GetEntryScriptHash")]
    SystemRuntimeGetEntryScriptHash,

    // Price: 1 << 10, CallFlags: NoneFlag, ParamCount: 1
    #[strum(serialize = "System.Runtime.CheckWitness")]
    SystemRuntimeCheckWitness,

    // Price: 1 << 4
    #[strum(serialize = "System.Runtime.GetInvocationCounter")]
    SystemRuntimeGetInvocationCounter,

    // Price: 1 << 15, CallFlags: AllowNotify, ParamCount: 1
    #[strum(serialize = "System.Runtime.Log")]
    SystemRuntimeLog,

    // Price: 1 << 12, ParamCount: 1
    #[strum(serialize = "System.Runtime.GetNotifications")]
    SystemRuntimeGetNotifications,

    // Price: 1 << 4
    #[strum(serialize = "System.Runtime.GasLeft")]
    SystemRuntimeGasLeft,

    // Price: 1 << 4, ParamCount: 1
    #[strum(serialize = "System.Runtime.BurnGas")]
    SystemRuntimeBurnGas,

    // Price: 1 << 3
    #[strum(serialize = "System.Runtime.GetNetwork")]
    SystemRuntimeGetNetwork,

    // Price: 0
    #[strum(serialize = "System.Runtime.GetRandom")]
    SystemRuntimeGetRandom,

    // Price: 1 << 4, CallFlags: NoneFlag
    #[strum(serialize = "System.Runtime.CurrentSigners")]
    SystemRuntimeCurrentSigners,

    // Price: 1 << 3
    #[strum(serialize = "System.Runtime.GetAddressVersion")]
    SystemRuntimeGetAddressVersion,

    // Price: 1 << 15, CallFlags: AllowCall, ParamCount: 3
    #[strum(serialize = "System.Runtime.LoadScript")]
    SystemRuntimeLoadScript,

    // Price: 1 << 15, CallFlags: AllowNotify, ParamCount: 2
    #[strum(serialize = "System.Runtime.Notify")]
    SystemRuntimeNotify,

    // === Storage ===

    // Price: 1 << 4, CallFlags: ReadStates
    #[strum(serialize = "System.Storage.GetContext")]
    SystemStorageGetContext,

    // Price: 1 << 4, CallFlags: ReadStates
    #[strum(serialize = "System.Storage.GetReadOnlyContext")]
    SystemStorageGetReadOnlyContext,

    // Price: 1 << 4, CallFlags: ReadStates, ParamCount: 1
    #[strum(serialize = "System.StorageContext.AsReadOnly")]
    SystemStorageAsReadOnly,

    // Price: 1 << 15, CallFlags: ReadStates, ParamCount: 2
    #[strum(serialize = "System.Storage.Get")]
    SystemStorageGet,

    // Price: 1 << 15, CallFlags: ReadStates, ParamCount: 3
    #[strum(serialize = "System.Storage.Find")]
    SystemStorageFind,

    // Price: 1 << 15, CallFlags: WriteStates, ParamCount: 3
    #[strum(serialize = "System.Storage.Put")]
    SystemStoragePut,

    // Price: 1 << 15, CallFlags: WriteStates, ParamCount: 2
    #[strum(serialize = "System.Storage.Delete")]
    SystemStorageDelete,
}

#[derive(Debug, Copy, Clone)]
pub struct CallAttr {
    pub call_flags: CallFlags,
    pub nr_params: u32,
    pub price: u64,
}

impl InteropCall {
    pub const fn as_u8(&self) -> u8 { *self as u8 }

    pub fn attr(&self) -> CallAttr {
        use CallFlags::*;
        match self {
            SystemContractCall => CallAttr { price: 1 << 15, call_flags: ReadOnly, nr_params: 4 },
            SystemContractCallNative => CallAttr { price: 0, call_flags: None, nr_params: 1 },
            SystemContractCreateMultiSigAccount => { CallAttr { price: 0, call_flags: None, nr_params: 2 } }
            SystemContractCreateStandardAccount => { CallAttr { price: 0, call_flags: None, nr_params: 1 } }
            SystemContractGetCallFlags => { CallAttr { price: 1 << 10, call_flags: None, nr_params: 0 } }
            SystemContractNativeOnPersist => { CallAttr { price: 0, call_flags: States, nr_params: 0 } }
            SystemContractNativePostPersist => { CallAttr { price: 0, call_flags: States, nr_params: 0 } }
            SystemCryptoCheckMultiSig => CallAttr { price: 0, call_flags: None, nr_params: 2 },
            SystemCryptoCheckSig => CallAttr { price: 1 << 15, call_flags: None, nr_params: 2 },
            SystemIteratorNext => CallAttr { price: 1 << 15, call_flags: None, nr_params: 1 },
            SystemIteratorValue => CallAttr { price: 1 << 4, call_flags: None, nr_params: 1 },
            SystemRuntimeBurnGas => CallAttr { price: 1 << 4, call_flags: None, nr_params: 1 },
            SystemRuntimeCheckWitness => { CallAttr { price: 1 << 10, call_flags: None, nr_params: 1 } }
            SystemRuntimeCurrentSigners => { CallAttr { price: 1 << 4, call_flags: None, nr_params: 0 } }
            SystemRuntimeGasLeft => CallAttr { price: 1 << 4, call_flags: None, nr_params: 0 },
            SystemRuntimeGetAddressVersion => { CallAttr { price: 1 << 3, call_flags: None, nr_params: 0 } }
            SystemRuntimeGetCallingScriptHash => { CallAttr { price: 1 << 4, call_flags: None, nr_params: 0 } }
            SystemRuntimeGetEntryScriptHash => { CallAttr { price: 1 << 4, call_flags: None, nr_params: 0 } }
            SystemRuntimeGetExecutingScriptHash => { CallAttr { price: 1 << 4, call_flags: None, nr_params: 0 } }
            SystemRuntimeGetInvocationCounter => { CallAttr { price: 1 << 4, call_flags: None, nr_params: 0 } }
            SystemRuntimeGetNetwork => CallAttr { price: 1 << 3, call_flags: None, nr_params: 0 },
            SystemRuntimeGetNotifications => { CallAttr { price: 1 << 12, call_flags: None, nr_params: 1 } }
            SystemRuntimeGetRandom => CallAttr { price: 0, call_flags: None, nr_params: 0 },
            SystemRuntimeGetScriptContainer => { CallAttr { price: 1 << 3, call_flags: None, nr_params: 0 } }
            SystemRuntimeGetTime => { CallAttr { price: 1 << 3, call_flags: ReadStates, nr_params: 0 } }
            SystemRuntimeGetTrigger => CallAttr { price: 1 << 3, call_flags: None, nr_params: 0 },
            SystemRuntimeLoadScript => { CallAttr { price: 1 << 15, call_flags: AllowCall, nr_params: 3 } }
            SystemRuntimeLog => CallAttr { price: 1 << 15, call_flags: AllowNotify, nr_params: 1 },
            SystemRuntimeNotify => { CallAttr { price: 1 << 15, call_flags: AllowNotify, nr_params: 2 } }
            SystemRuntimePlatform => CallAttr { price: 1 << 3, call_flags: None, nr_params: 0 },
            SystemStorageDelete => { CallAttr { price: 1 << 15, call_flags: WriteStates, nr_params: 2 } }
            SystemStorageFind => CallAttr { price: 1 << 15, call_flags: ReadStates, nr_params: 3 },
            SystemStorageGet => CallAttr { price: 1 << 15, call_flags: ReadStates, nr_params: 2 },
            SystemStorageGetContext => { CallAttr { price: 1 << 4, call_flags: ReadStates, nr_params: 0 } }
            SystemStorageGetReadOnlyContext => { CallAttr { price: 1 << 4, call_flags: ReadStates, nr_params: 0 } }
            SystemStoragePut => CallAttr { price: 1 << 15, call_flags: WriteStates, nr_params: 3 },
            SystemStorageAsReadOnly => { CallAttr { price: 1 << 4, call_flags: ReadStates, nr_params: 1 } }
        }
    }
}

impl RunPrice for InteropCall {
    #[inline]
    fn price(&self) -> u64 { self.attr().price }
}

#[cfg(test)]
mod test {
    use strum::IntoEnumIterator;

    use super::*;

    #[test]
    fn test_interop_call_price() {
        const CALL_UPPER: u8 = 38;
        for call in InteropCall::iter() {
            assert_eq!(call.attr().price, call.price());
            assert!(call.as_u8() < CALL_UPPER);
        }

        assert_eq!(SystemContractCall.as_u8(), 0);
    }
}
