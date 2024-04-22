// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use strum_macros::{EnumString, Display};

use crate::RunPrice;


#[derive(EnumString, Display, Copy, Clone, PartialEq, Eq)]
pub enum InteropService {
    #[strum(serialize = "System.Crypto.CheckSig")]
    SystemCryptoCheckSig,

    #[strum(serialize = "System.Crypto.CheckMultiSig")]
    SystemCryptoCheckMultiSig,

    #[strum(serialize = "System.Contract.Call")]
    SystemContractCall,

    #[strum(serialize = "System.Contract.CallNative")]
    SystemContractCallNative,

    #[strum(serialize = "System.Contract.GetCallFlags")]
    SystemContractGetCallFlags,

    #[strum(serialize = "System.Contract.CreateStandardAccount")]
    SystemContractCreateStandardAccount,

    #[strum(serialize = "System.Contract.CreateMultiSigAccount")]
    SystemContractCreateMultiSigAccount,

    #[strum(serialize = "System.Contract.NativeOnPersist")]
    SystemContractNativeOnPersist,

    #[strum(serialize = "System.Contract.NativePostPersist")]
    SystemContractNativePostPersist,

    #[strum(serialize = "System.Iterator.Next")]
    SystemIteratorNext,

    #[strum(serialize = "System.Iterator.Value")]
    SystemIteratorValue,

    #[strum(serialize = "System.Runtime.Platform")]
    SystemRuntimePlatform,

    #[strum(serialize = "System.Runtime.GetTrigger")]
    SystemRuntimeGetTrigger,

    #[strum(serialize = "System.Runtime.GetTime")]
    SystemRuntimeGetTime,

    #[strum(serialize = "System.Runtime.GetScriptContainer")]
    SystemRuntimeGetScriptContainer,

    #[strum(serialize = "System.Runtime.GetExecutingScriptHash")]
    SystemRuntimeGetExecutingScriptHash,

    #[strum(serialize = "System.Runtime.GetCallingScriptHash")]
    SystemRuntimeGetCallingScriptHash,

    #[strum(serialize = "System.Runtime.GetEntryScriptHash")]
    SystemRuntimeGetEntryScriptHash,

    #[strum(serialize = "System.Runtime.CheckWitness")]
    SystemRuntimeCheckWitness,

    #[strum(serialize = "System.Runtime.GetInvocationCounter")]
    SystemRuntimeGetInvocationCounter,

    #[strum(serialize = "System.Runtime.Log")]
    SystemRuntimeLog,

    #[strum(serialize = "System.Runtime.Notify")]
    SystemRuntimeNotify,

    #[strum(serialize = "System.Runtime.GetNotifications")]
    SystemRuntimeGetNotifications,

    #[strum(serialize = "System.Runtime.GasLeft")]
    SystemRuntimeGasLeft,
    #[strum(serialize = "System.Runtime.BurnGas")]
    SystemRuntimeBurnGas,

    #[strum(serialize = "System.Runtime.GetNetwork")]
    SystemRuntimeGetNetwork,

    #[strum(serialize = "System.Runtime.GetRandom")]
    SystemRuntimeGetRandom,

    #[strum(serialize = "System.Storage.GetContext")]
    SystemStorageGetContext,

    #[strum(serialize = "System.Storage.GetReadOnlyContext")]
    SystemStorageGetReadOnlyContext,

    #[strum(serialize = "System.Storage.AsReadOnly")]
    SystemStorageAsReadOnly,

    #[strum(serialize = "System.Storage.Get")]
    SystemStorageGet,

    #[strum(serialize = "System.Storage.Find")]
    SystemStorageFind,

    #[strum(serialize = "System.Storage.Put")]
    SystemStoragePut,

    #[strum(serialize = "System.Storage.Delete")]
    SystemStorageDelete,
}


impl RunPrice for InteropService {
    fn price(&self) -> u64 {
        match self {
            Self::SystemRuntimePlatform |
            Self::SystemRuntimeGetTrigger |
            Self::SystemRuntimeGetTime |
            Self::SystemRuntimeGetScriptContainer |
            Self::SystemRuntimeGetNetwork => 1 << 3,

            Self::SystemIteratorValue |
            Self::SystemRuntimeGetExecutingScriptHash |
            Self::SystemRuntimeGetCallingScriptHash |
            Self::SystemRuntimeGetEntryScriptHash |
            Self::SystemRuntimeGetInvocationCounter |
            Self::SystemRuntimeGasLeft |
            Self::SystemRuntimeBurnGas |
            Self::SystemRuntimeGetRandom |
            Self::SystemStorageGetContext |
            Self::SystemStorageGetReadOnlyContext |
            Self::SystemStorageAsReadOnly => 1 << 4,

            Self::SystemContractGetCallFlags |
            Self::SystemRuntimeCheckWitness => 1 << 10,

            Self::SystemRuntimeGetNotifications => 1 << 12,

            Self::SystemCryptoCheckSig |
            Self::SystemContractCall |
            Self::SystemContractCreateStandardAccount |
            Self::SystemIteratorNext |
            Self::SystemRuntimeLog |
            Self::SystemRuntimeNotify |
            Self::SystemStorageGet |
            Self::SystemStorageFind |
            Self::SystemStoragePut |
            Self::SystemStorageDelete => 1 << 15,
            _ => 0,
        }
    }
}
