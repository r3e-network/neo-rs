use std::sync::LazyLock;

use neo_execution::NativeMethod;
use neo_primitives::{CallFlags, ContractParameterType};

pub(super) static NOTARY_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    let int = ContractParameterType::Integer;
    vec![
        NativeMethod::new(
            "getMaxNotValidBeforeDelta",
            1 << 15,
            true,
            read_states,
            vec![],
            int,
        ),
        // Deposit reads: balanceOf -> Amount, expirationOf -> Till.
        NativeMethod::new(
            "balanceOf",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            int,
        )
        .with_parameter_names(["account"]),
        NativeMethod::new(
            "expirationOf",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            int,
        )
        .with_parameter_names(["account"]),
        // Committee-gated setter: not safe, States, Integer -> Void.
        NativeMethod::new(
            "setMaxNotValidBeforeDelta",
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![int],
            ContractParameterType::Void,
        )
        .with_parameter_names(["value"]),
        // lockDepositUntil(account, till) -> bool: account-witnessed, States.
        NativeMethod::new(
            "lockDepositUntil",
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Hash160, int],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["account", "till"]),
        // onNEP17Payment(from, amount, data) -> Void: GAS deposit callback, States.
        crate::nep17_payment_method(1 << 15, false, CallFlags::STATES.bits()),
        // withdraw(from, to?) -> bool: depositor-witnessed; transfers the unlocked
        // deposit GAS from Notary to `to` (re-entrant, CallFlags.All).
        NativeMethod::new(
            "withdraw",
            1 << 15,
            false,
            CallFlags::ALL.bits(),
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::Hash160,
            ],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["from", "to"]),
        // verify(signature) -> bool: notary-witness verification. C#
        // `[ContractMethod(CpuFee = 1 << 15, RequiredCallFlags = CallFlags.ReadStates)]`
        // (Notary.cs Verify), and ContractMethodMetadata derives
        // `Safe = (ReadStates & ~CallFlags.ReadOnly) == 0` -> manifest-safe.
        NativeMethod::new(
            "verify",
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::ByteArray],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["signature"]),
    ]
});
