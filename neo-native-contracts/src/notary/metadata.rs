use std::sync::LazyLock;

use neo_execution::NativeMethod;
use neo_primitives::{CallFlags, ContractParameterType};

use super::Notary;
use crate::support::invoke::{NativeMethodBinding, method_metadata};

pub(super) static NOTARY_METHOD_BINDINGS: LazyLock<Vec<NativeMethodBinding<Notary>>> =
    LazyLock::new(|| {
        let read_states = CallFlags::READ_STATES.bits();
        let int = ContractParameterType::Integer;
        vec![
            NativeMethodBinding::new(
                NativeMethod::new(
                    "getMaxNotValidBeforeDelta",
                    1 << 15,
                    true,
                    read_states,
                    vec![],
                    int,
                ),
                Notary::invoke_get_max_not_valid_before_delta,
            ),
            // Deposit reads: balanceOf -> Amount, expirationOf -> Till.
            NativeMethodBinding::new(
                NativeMethod::new(
                    "balanceOf",
                    1 << 15,
                    true,
                    read_states,
                    vec![ContractParameterType::Hash160],
                    int,
                )
                .with_parameter_names(["account"]),
                Notary::invoke_balance_of,
            ),
            NativeMethodBinding::new(
                NativeMethod::new(
                    "expirationOf",
                    1 << 15,
                    true,
                    read_states,
                    vec![ContractParameterType::Hash160],
                    int,
                )
                .with_parameter_names(["account"]),
                Notary::invoke_expiration_of,
            ),
            // Committee-gated setter: not safe, States, Integer -> Void.
            NativeMethodBinding::new(
                NativeMethod::new(
                    "setMaxNotValidBeforeDelta",
                    1 << 15,
                    false,
                    CallFlags::STATES.bits(),
                    vec![int],
                    ContractParameterType::Void,
                )
                .with_parameter_names(["value"]),
                Notary::invoke_set_max_not_valid_before_delta,
            ),
            // lockDepositUntil(account, till) -> bool: account-witnessed, States.
            NativeMethodBinding::new(
                NativeMethod::new(
                    "lockDepositUntil",
                    1 << 15,
                    false,
                    CallFlags::STATES.bits(),
                    vec![ContractParameterType::Hash160, int],
                    ContractParameterType::Boolean,
                )
                .with_parameter_names(["account", "till"]),
                Notary::invoke_lock_deposit_until,
            ),
            // onNEP17Payment(from, amount, data) -> Void: GAS deposit callback, States.
            NativeMethodBinding::new(
                crate::nep17_payment_method(1 << 15, false, CallFlags::STATES.bits()),
                Notary::invoke_on_nep17_payment,
            ),
            // withdraw(from, to?) -> bool: depositor-witnessed; transfers the unlocked
            // deposit GAS from Notary to `to` (re-entrant, CallFlags.All).
            NativeMethodBinding::new(
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
                Notary::invoke_withdraw,
            ),
            // verify(signature) -> bool: notary-witness verification. C#
            // `[ContractMethod(CpuFee = 1 << 15, RequiredCallFlags = CallFlags.ReadStates)]`
            // (Notary.cs Verify), and ContractMethodMetadata derives
            // `Safe = (ReadStates & ~CallFlags.ReadOnly) == 0` -> manifest-safe.
            NativeMethodBinding::new(
                NativeMethod::new(
                    "verify",
                    1 << 15,
                    true,
                    read_states,
                    vec![ContractParameterType::ByteArray],
                    ContractParameterType::Boolean,
                )
                .with_parameter_names(["signature"]),
                Notary::invoke_verify,
            ),
        ]
    });

pub(super) static NOTARY_METHODS: LazyLock<Vec<NativeMethod>> =
    LazyLock::new(|| method_metadata(&NOTARY_METHOD_BINDINGS));
