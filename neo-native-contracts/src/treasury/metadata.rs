use std::sync::LazyLock;

use neo_execution::NativeMethod;
use neo_primitives::{CallFlags, ContractParameterType};

use super::Treasury;
use crate::support::invoke::{NativeMethodBinding, method_metadata};

pub(super) fn treasury_method_bindings<P, D, B>() -> Vec<NativeMethodBinding<Treasury, P, D, B>>
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: neo_execution::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    // C# `[ContractMethod(CpuFee = 1 << 5, RequiredCallFlags = CallFlags.None)]`
    // (Treasury.cs OnNEP17Payment/OnNEP11Payment). ContractMethodMetadata
    // derives `Safe = (None & ~CallFlags.ReadOnly) == 0`, so both payment
    // callbacks are manifest-safe (unlike Notary's, which requires States).
    vec![
        NativeMethodBinding::new(
            crate::nep17_payment_method(1 << 5, true, 0),
            Treasury::invoke_nep_payment,
        ),
        NativeMethodBinding::new(
            crate::nep11_payment_method(1 << 5, true, 0),
            Treasury::invoke_nep_payment,
        ),
        // C# `[ContractMethod(CpuFee = 1 << 5, RequiredCallFlags =
        // CallFlags.ReadStates)] private bool Verify(ApplicationEngine engine)`
        // (Treasury.cs:41-42): ReadStates is a subset of ReadOnly, so it is
        // manifest-safe.
        NativeMethodBinding::new(
            NativeMethod::new(
                "verify",
                1 << 5,
                true,
                CallFlags::READ_STATES.bits(),
                vec![],
                ContractParameterType::Boolean,
            ),
            Treasury::invoke_verify,
        ),
    ]
}

pub(super) static TREASURY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    method_metadata(&treasury_method_bindings::<
        neo_execution::native_contract_provider::NoNativeContractProvider,
        neo_execution::NoDiagnostic,
        neo_storage::EmptyCacheBacking,
    >())
});
