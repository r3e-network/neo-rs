use std::sync::LazyLock;

use neo_execution::{NativeEvent, NativeMethod};
use neo_primitives::CallFlags;

pub(super) static GAS_TOKEN_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        // NEP-17 metadata: `[ContractMethod]` with no CpuFee -> fee 0, no flags.
        crate::nep17_symbol_method(),
        crate::nep17_decimals_method(),
        // NEP-17 state reads: CpuFee 1<<15, RequiredCallFlags ReadStates.
        crate::nep17_total_supply_method(read_states),
        crate::nep17_balance_of_method(read_states),
        // NEP-17 transfer: CpuFee 1<<17, StorageFee 50, States|AllowCall|AllowNotify,
        // (from, to, amount, data) -> Boolean. Not safe.
        crate::nep17_transfer_method(),
    ]
});

/// GAS declares no events of its own; the only manifest event is the
/// `Transfer` inherited from the C# `FungibleToken` base constructor.
pub(super) static GAS_TOKEN_EVENTS: LazyLock<Vec<NativeEvent>> =
    LazyLock::new(|| vec![crate::fungible_token_transfer_event()]);
