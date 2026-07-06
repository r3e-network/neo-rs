use neo_config::Hardfork;
use neo_execution::{NativeEvent, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType};
use std::sync::LazyLock;

use super::{
    POLICY_MILLISECONDS_PER_BLOCK_CHANGED_EVENT, POLICY_RECOVERED_FUND_EVENT,
    POLICY_WHITELIST_FEE_CHANGED_EVENT, PolicyContract,
};
use crate::support::invoke::{NativeMethodBinding, method_metadata};

pub(super) static POLICY_CONTRACT_METHOD_BINDINGS: LazyLock<
    Vec<NativeMethodBinding<PolicyContract>>,
> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethodBinding::new(
            NativeMethod::new(
                "getFeePerByte",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::Integer,
            ),
            PolicyContract::invoke_get_fee_per_byte,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getStoragePrice",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::Integer,
            ),
            PolicyContract::invoke_get_storage_price,
        ),
        // Committee-gated setters: not safe, require write (States) call flags.
        NativeMethodBinding::new(
            NativeMethod::new(
                "setFeePerByte",
                1 << 15,
                false,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(["value"]),
            PolicyContract::invoke_set_fee_per_byte,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "setStoragePrice",
                1 << 15,
                false,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(["value"]),
            PolicyContract::invoke_set_storage_price,
        ),
        // Execution fee factor: getExecFeeFactor (always present; divides out the
        // HF_Faun pico-GAS scaling), getExecPicoFeeFactor (HF_Faun; raw pico-GAS),
        // and the committee-gated setExecFeeFactor.
        NativeMethodBinding::new(
            NativeMethod::new(
                "getExecFeeFactor",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::Integer,
            ),
            PolicyContract::invoke_get_exec_fee_factor,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getExecPicoFeeFactor",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfFaun),
            PolicyContract::invoke_get_exec_pico_fee_factor,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "setExecFeeFactor",
                1 << 15,
                false,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_parameter_names(["value"]),
            PolicyContract::invoke_set_exec_fee_factor,
        ),
        // getAttributeFee / setAttributeFee: dual C# descriptor registrations.
        // V0 is genesis-active and DeprecatedIn HF_Echidna; V1 is ActiveIn
        // HF_Echidna. The ABI signature is identical across versions, but the
        // native method cache and hardfork-gated descriptors should stay
        // literal to C#.
        NativeMethodBinding::new(
            NativeMethod::new(
                "getAttributeFee",
                1 << 15,
                true,
                read_states,
                vec![ContractParameterType::Integer],
                ContractParameterType::Integer,
            )
            .with_deprecated_in(Hardfork::HfEchidna)
            .with_parameter_names(["attributeType"]),
            PolicyContract::invoke_get_attribute_fee,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getAttributeFee",
                1 << 15,
                true,
                read_states,
                vec![ContractParameterType::Integer],
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(["attributeType"]),
            PolicyContract::invoke_get_attribute_fee,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "setAttributeFee",
                1 << 15,
                false,
                CallFlags::STATES.bits(),
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_deprecated_in(Hardfork::HfEchidna)
            .with_parameter_names(["attributeType", "value"]),
            PolicyContract::invoke_set_attribute_fee,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "setAttributeFee",
                1 << 15,
                false,
                CallFlags::STATES.bits(),
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(["attributeType", "value"]),
            PolicyContract::invoke_set_attribute_fee,
        ),
        // getBlockedAccounts() -> Iterator over blocked account hashes (HF_Faun).
        NativeMethodBinding::new(
            NativeMethod::new(
                "getBlockedAccounts",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::InteropInterface,
            )
            .with_active_in(Hardfork::HfFaun),
            PolicyContract::invoke_get_blocked_accounts,
        ),
        // HF_Echidna setter that emits a change notification (States|AllowNotify).
        NativeMethodBinding::new(
            NativeMethod::new(
                "setMillisecondsPerBlock",
                1 << 15,
                false,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(["value"]),
            PolicyContract::invoke_set_milliseconds_per_block,
        ),
        // HF_Echidna chain-parameter setters with cross-value invariants (States).
        NativeMethodBinding::new(
            NativeMethod::new(
                "setMaxValidUntilBlockIncrement",
                1 << 15,
                false,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(["value"]),
            PolicyContract::invoke_set_max_valid_until_block_increment,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "setMaxTraceableBlocks",
                1 << 15,
                false,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Integer],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfEchidna)
            .with_parameter_names(["value"]),
            PolicyContract::invoke_set_max_traceable_blocks,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "isBlocked",
                1 << 15,
                true,
                read_states,
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_parameter_names(["account"]),
            PolicyContract::invoke_is_blocked,
        ),
        // Committee-gated unblock writer (not safe, States, Boolean return).
        NativeMethodBinding::new(
            NativeMethod::new(
                "unblockAccount",
                1 << 15,
                false,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_parameter_names(["account"]),
            PolicyContract::invoke_unblock_account,
        ),
        // HF_Echidna moved these chain parameters from ProtocolSettings into
        // PolicyContract storage; the getters default to the settings value.
        NativeMethodBinding::new(
            NativeMethod::new(
                "getMillisecondsPerBlock",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna),
            PolicyContract::invoke_get_milliseconds_per_block,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getMaxValidUntilBlockIncrement",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna),
            PolicyContract::invoke_get_max_valid_until_block_increment,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getMaxTraceableBlocks",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::Integer,
            )
            .with_active_in(Hardfork::HfEchidna),
            PolicyContract::invoke_get_max_traceable_blocks,
        ),
        // blockAccount: dual manifest registration under one name (C# V0/V1).
        // V0 = ContractMethod(true, HF_Faun): genesis-active, DeprecatedIn Faun,
        // flags States. V1 = ActiveIn HF_Faun, flags States|AllowNotify (the
        // Faun path emits NEO's Vote notification via VoteInternal). Exactly one
        // is active at any height, so the manifest/dispatcher never sees both.
        NativeMethodBinding::new(
            NativeMethod::new(
                "blockAccount",
                1 << 15,
                false,
                CallFlags::STATES.bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_deprecated_in(Hardfork::HfFaun)
            .with_parameter_names(["account"]),
            PolicyContract::invoke_block_account,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "blockAccount",
                1 << 15,
                false,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![ContractParameterType::Hash160],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(["account"]),
            PolicyContract::invoke_block_account,
        ),
        // Whitelisted fixed-fee contracts (HF_Faun): committee-gated writers
        // that notify WhitelistFeeChanged, plus the safe iterator reader.
        NativeMethodBinding::new(
            NativeMethod::new(
                "setWhitelistFeeContract",
                1 << 15,
                false,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::String,
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(["contractHash", "method", "argCount", "fixedFee"]),
            PolicyContract::invoke_set_whitelist_fee_contract,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "removeWhitelistFeeContract",
                1 << 15,
                false,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::String,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Void,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(["contractHash", "method", "argCount"]),
            PolicyContract::invoke_remove_whitelist_fee_contract,
        ),
        NativeMethodBinding::new(
            NativeMethod::new(
                "getWhitelistFeeContracts",
                1 << 15,
                true,
                read_states,
                vec![],
                ContractParameterType::InteropInterface,
            )
            .with_active_in(Hardfork::HfFaun),
            PolicyContract::invoke_get_whitelist_fee_contracts,
        ),
        // recoverFund(account, token) -> Boolean (HF_Faun): an almost-full
        // committee sweep of a long-blocked account's NEP-17 funds to Treasury.
        NativeMethodBinding::new(
            NativeMethod::new(
                "recoverFund",
                1 << 15,
                false,
                // C# v3.10.0 `PolicyContract.RecoverFund` requires CallFlags.All;
                // the AllowCall bit gates the nested NEP-17 balanceOf/transfer
                // calls before Policy dispatches.
                CallFlags::ALL.bits(),
                vec![
                    ContractParameterType::Hash160,
                    ContractParameterType::Hash160,
                ],
                ContractParameterType::Boolean,
            )
            .with_active_in(Hardfork::HfFaun)
            .with_parameter_names(["account", "token"]),
            PolicyContract::invoke_recover_fund,
        ),
    ]
});

pub(super) static POLICY_CONTRACT_METHODS: LazyLock<Vec<NativeMethod>> =
    LazyLock::new(|| method_metadata(&POLICY_CONTRACT_METHOD_BINDINGS));

/// Policy's `[ContractEvent]` declarations (PolicyContract.cs:115-125), all
/// hardfork-gated: `MillisecondsPerBlockChanged` from `HF_Echidna`,
/// `WhitelistFeeChanged` and `RecoveredFund` from `HF_Faun`. (The C# names
/// come from the `*EventName` constants at PolicyContract.cs:111-113.)
pub(super) static POLICY_CONTRACT_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        NativeEvent::new(
            0,
            POLICY_MILLISECONDS_PER_BLOCK_CHANGED_EVENT,
            &[
                ("old", ContractParameterType::Integer),
                ("new", ContractParameterType::Integer),
            ],
        )
        .with_active_in(Hardfork::HfEchidna),
        NativeEvent::new(
            1,
            POLICY_WHITELIST_FEE_CHANGED_EVENT,
            &[
                ("contract", ContractParameterType::Hash160),
                ("method", ContractParameterType::String),
                ("argCount", ContractParameterType::Integer),
                ("fee", ContractParameterType::Any),
            ],
        )
        .with_active_in(Hardfork::HfFaun),
        NativeEvent::new(
            2,
            POLICY_RECOVERED_FUND_EVENT,
            &[("account", ContractParameterType::Hash160)],
        )
        .with_active_in(Hardfork::HfFaun),
    ]
});
