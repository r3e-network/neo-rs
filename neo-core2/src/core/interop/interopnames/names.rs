pub const SYSTEM_CONTRACT_CALL: &str = "System.Contract.Call";
pub const SYSTEM_CONTRACT_CALL_NATIVE: &str = "System.Contract.CallNative";
pub const SYSTEM_CONTRACT_CREATE_MULTISIG_ACCOUNT: &str = "System.Contract.CreateMultisigAccount";
pub const SYSTEM_CONTRACT_CREATE_STANDARD_ACCOUNT: &str = "System.Contract.CreateStandardAccount";
pub const SYSTEM_CONTRACT_GET_CALL_FLAGS: &str = "System.Contract.GetCallFlags";
pub const SYSTEM_CONTRACT_NATIVE_ON_PERSIST: &str = "System.Contract.NativeOnPersist";
pub const SYSTEM_CONTRACT_NATIVE_POST_PERSIST: &str = "System.Contract.NativePostPersist";
pub const SYSTEM_CRYPTO_CHECK_SIG: &str = "System.Crypto.CheckSig";
pub const SYSTEM_CRYPTO_CHECK_MULTISIG: &str = "System.Crypto.CheckMultisig";
pub const SYSTEM_ITERATOR_NEXT: &str = "System.Iterator.Next";
pub const SYSTEM_ITERATOR_VALUE: &str = "System.Iterator.Value";
pub const SYSTEM_RUNTIME_BURN_GAS: &str = "System.Runtime.BurnGas";
pub const SYSTEM_RUNTIME_CHECK_WITNESS: &str = "System.Runtime.CheckWitness";
pub const SYSTEM_RUNTIME_CURRENT_SIGNERS: &str = "System.Runtime.CurrentSigners";
pub const SYSTEM_RUNTIME_GAS_LEFT: &str = "System.Runtime.GasLeft";
pub const SYSTEM_RUNTIME_GET_ADDRESS_VERSION: &str = "System.Runtime.GetAddressVersion";
pub const SYSTEM_RUNTIME_GET_CALLING_SCRIPT_HASH: &str = "System.Runtime.GetCallingScriptHash";
pub const SYSTEM_RUNTIME_GET_ENTRY_SCRIPT_HASH: &str = "System.Runtime.GetEntryScriptHash";
pub const SYSTEM_RUNTIME_GET_EXECUTING_SCRIPT_HASH: &str = "System.Runtime.GetExecutingScriptHash";
pub const SYSTEM_RUNTIME_GET_INVOCATION_COUNTER: &str = "System.Runtime.GetInvocationCounter";
pub const SYSTEM_RUNTIME_GET_NETWORK: &str = "System.Runtime.GetNetwork";
pub const SYSTEM_RUNTIME_GET_NOTIFICATIONS: &str = "System.Runtime.GetNotifications";
pub const SYSTEM_RUNTIME_GET_RANDOM: &str = "System.Runtime.GetRandom";
pub const SYSTEM_RUNTIME_GET_SCRIPT_CONTAINER: &str = "System.Runtime.GetScriptContainer";
pub const SYSTEM_RUNTIME_GET_TIME: &str = "System.Runtime.GetTime";
pub const SYSTEM_RUNTIME_GET_TRIGGER: &str = "System.Runtime.GetTrigger";
pub const SYSTEM_RUNTIME_LOAD_SCRIPT: &str = "System.Runtime.LoadScript";
pub const SYSTEM_RUNTIME_LOG: &str = "System.Runtime.Log";
pub const SYSTEM_RUNTIME_NOTIFY: &str = "System.Runtime.Notify";
pub const SYSTEM_RUNTIME_PLATFORM: &str = "System.Runtime.Platform";
pub const SYSTEM_STORAGE_DELETE: &str = "System.Storage.Delete";
pub const SYSTEM_STORAGE_FIND: &str = "System.Storage.Find";
pub const SYSTEM_STORAGE_GET: &str = "System.Storage.Get";
pub const SYSTEM_STORAGE_GET_CONTEXT: &str = "System.Storage.GetContext";
pub const SYSTEM_STORAGE_GET_READ_ONLY_CONTEXT: &str = "System.Storage.GetReadOnlyContext";
pub const SYSTEM_STORAGE_PUT: &str = "System.Storage.Put";
pub const SYSTEM_STORAGE_AS_READ_ONLY: &str = "System.Storage.AsReadOnly";

pub const NAMES: &[&str] = &[
    SYSTEM_CONTRACT_CALL,
    SYSTEM_CONTRACT_CALL_NATIVE,
    SYSTEM_CONTRACT_CREATE_MULTISIG_ACCOUNT,
    SYSTEM_CONTRACT_CREATE_STANDARD_ACCOUNT,
    SYSTEM_CONTRACT_GET_CALL_FLAGS,
    SYSTEM_CONTRACT_NATIVE_ON_PERSIST,
    SYSTEM_CONTRACT_NATIVE_POST_PERSIST,
    SYSTEM_ITERATOR_NEXT,
    SYSTEM_ITERATOR_VALUE,
    SYSTEM_RUNTIME_BURN_GAS,
    SYSTEM_RUNTIME_CHECK_WITNESS,
    SYSTEM_RUNTIME_CURRENT_SIGNERS,
    SYSTEM_RUNTIME_GAS_LEFT,
    SYSTEM_RUNTIME_GET_ADDRESS_VERSION,
    SYSTEM_RUNTIME_GET_CALLING_SCRIPT_HASH,
    SYSTEM_RUNTIME_GET_ENTRY_SCRIPT_HASH,
    SYSTEM_RUNTIME_GET_EXECUTING_SCRIPT_HASH,
    SYSTEM_RUNTIME_GET_INVOCATION_COUNTER,
    SYSTEM_RUNTIME_GET_NETWORK,
    SYSTEM_RUNTIME_GET_NOTIFICATIONS,
    SYSTEM_RUNTIME_GET_RANDOM,
    SYSTEM_RUNTIME_GET_SCRIPT_CONTAINER,
    SYSTEM_RUNTIME_GET_TIME,
    SYSTEM_RUNTIME_GET_TRIGGER,
    SYSTEM_RUNTIME_LOG,
    SYSTEM_RUNTIME_NOTIFY,
    SYSTEM_RUNTIME_PLATFORM,
    SYSTEM_STORAGE_DELETE,
    SYSTEM_STORAGE_FIND,
    SYSTEM_STORAGE_GET,
    SYSTEM_STORAGE_GET_CONTEXT,
    SYSTEM_STORAGE_GET_READ_ONLY_CONTEXT,
    SYSTEM_STORAGE_PUT,
    SYSTEM_STORAGE_AS_READ_ONLY,
    SYSTEM_CRYPTO_CHECK_MULTISIG,
    SYSTEM_CRYPTO_CHECK_SIG,
];
