use std::sync::LazyLock;

use neo_execution::{NativeEvent, NativeMethod};
use neo_primitives::CallFlags;

use super::GasToken;
use crate::support::invoke::{NativeMethodBinding, method_metadata};

pub(super) fn gas_token_method_bindings<P, D, B>() -> Vec<NativeMethodBinding<GasToken, P, D, B>>
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
    D: neo_execution::Diagnostic + 'static,
    B: neo_storage::CacheRead,
{
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        NativeMethodBinding::new(crate::nep17_symbol_method(), GasToken::invoke_symbol),
        NativeMethodBinding::new(crate::nep17_decimals_method(), GasToken::invoke_decimals),
        // NEP-17 state reads: CpuFee 1<<15, RequiredCallFlags ReadStates.
        NativeMethodBinding::new(
            crate::nep17_total_supply_method(read_states),
            GasToken::invoke_total_supply,
        ),
        NativeMethodBinding::new(
            crate::nep17_balance_of_method(read_states),
            GasToken::invoke_balance_of,
        ),
        // NEP-17 transfer: CpuFee 1<<17, StorageFee 50, States|AllowCall|AllowNotify,
        // (from, to, amount, data) -> Boolean. Not safe.
        NativeMethodBinding::new(crate::nep17_transfer_method(), GasToken::invoke_transfer),
    ]
}

pub(super) static GAS_TOKEN_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    method_metadata(&gas_token_method_bindings::<
        neo_execution::native_contract_provider::NoNativeContractProvider,
        neo_execution::NoDiagnostic,
        neo_storage::EmptyCacheBacking,
    >())
});

/// GAS declares no events of its own; the only manifest event is the
/// `Transfer` inherited from the C# `FungibleToken` base constructor.
pub(super) static GAS_TOKEN_EVENTS: LazyLock<Vec<NativeEvent>> =
    LazyLock::new(|| vec![crate::fungible_token_transfer_event()]);
