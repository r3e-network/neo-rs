use std::sync::LazyLock;

use neo_execution::NativeMethod;
use neo_primitives::{CallFlags, ContractParameterType};

pub(super) static TREASURY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    // C# `[ContractMethod(CpuFee = 1 << 5, RequiredCallFlags = CallFlags.None)]`
    // (Treasury.cs OnNEP17Payment/OnNEP11Payment). ContractMethodMetadata
    // derives `Safe = (None & ~CallFlags.ReadOnly) == 0`, so both payment
    // callbacks are manifest-safe (unlike Notary's, which requires States).
    vec![
        crate::nep17_payment_method(1 << 5, true, 0),
        crate::nep11_payment_method(1 << 5, true, 0),
        // C# `[ContractMethod(CpuFee = 1 << 5, RequiredCallFlags =
        // CallFlags.ReadStates)] private bool Verify(ApplicationEngine engine)`
        // (Treasury.cs:41-42): ReadStates is a subset of ReadOnly, so it is
        // manifest-safe.
        NativeMethod::new(
            "verify",
            1 << 5,
            true,
            CallFlags::READ_STATES.bits(),
            vec![],
            ContractParameterType::Boolean,
        ),
    ]
});
