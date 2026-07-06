use std::sync::LazyLock;

use neo_config::Hardfork;
use neo_execution::{NativeEvent, NativeMethod};
use neo_primitives::{CallFlags, ContractParameterType};

use super::{ROLE_DESIGNATION_EVENT, RoleManagement};
use crate::support::invoke::{NativeMethodBinding, method_metadata};

pub(super) static ROLE_MANAGEMENT_METHOD_BINDINGS: LazyLock<
    Vec<NativeMethodBinding<RoleManagement>>,
> = LazyLock::new(|| {
    vec![
        NativeMethodBinding::new(
            NativeMethod::new(
                "getDesignatedByRole",
                1 << 15,
                true,
                CallFlags::READ_STATES.bits(),
                vec![
                    ContractParameterType::Integer,
                    ContractParameterType::Integer,
                ],
                ContractParameterType::Array,
            )
            .with_parameter_names(["role", "index"]),
            RoleManagement::invoke_get_designated_by_role,
        ),
        // Committee-gated writer that emits a Designation event (States|AllowNotify).
        NativeMethodBinding::new(
            NativeMethod::new(
                "designateAsRole",
                1 << 15,
                false,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
                vec![ContractParameterType::Integer, ContractParameterType::Array],
                ContractParameterType::Void,
            )
            .with_parameter_names(["role", "nodes"]),
            RoleManagement::invoke_designate_as_role,
        ),
    ]
});

pub(super) static ROLE_MANAGEMENT_METHODS: LazyLock<Vec<NativeMethod>> =
    LazyLock::new(|| method_metadata(&ROLE_MANAGEMENT_METHOD_BINDINGS));

/// The dual `Designation` event registration (RoleManagement.cs:27-37): both
/// share order 0 and exactly one is active at any height. V0
/// `(Role, BlockIndex)` is genesis-active and DeprecatedIn `HF_Echidna`
/// (the trailing ctor argument); V1 adds the `Old`/`New` node arrays and is
/// ActiveIn `HF_Echidna`.
pub(super) static ROLE_MANAGEMENT_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        NativeEvent::new(
            0,
            ROLE_DESIGNATION_EVENT,
            &[
                ("Role", ContractParameterType::Integer),
                ("BlockIndex", ContractParameterType::Integer),
            ],
        )
        .with_deprecated_in(Hardfork::HfEchidna),
        NativeEvent::new(
            0,
            ROLE_DESIGNATION_EVENT,
            &[
                ("Role", ContractParameterType::Integer),
                ("BlockIndex", ContractParameterType::Integer),
                ("Old", ContractParameterType::Array),
                ("New", ContractParameterType::Array),
            ],
        )
        .with_active_in(Hardfork::HfEchidna),
    ]
});
