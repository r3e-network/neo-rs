//! Macros for compact native contract metadata declarations.
//!
//! Migrated from `neo-core/src/smart_contract/native/metadata_macros.rs`.
//!
//! `$crate` resolves to `neo-native-contracts` here, so the original
//! `neo-core` manifest paths are rewritten to their destination crates:
//!
//! | neo-core source path                                   | destination path                                  |
//!|--------------------------------------------------------|---------------------------------------------------|
//! | `$crate::smart_contract::manifest::ContractEventDescriptor`   | `neo_smart_contract::manifest::ContractEventDescriptor`   |
//! | `$crate::smart_contract::manifest::ContractParameterDefinition` | `neo_smart_contract::manifest::ContractParameterDefinition` |
//! | `$crate::smart_contract::ContractParameterType`        | `neo_primitives::ContractParameterType`           |

macro_rules! event_descriptor {
    (
        $name:literal,
        [$($param_name:literal => $param_type:ident),* $(,)?]
    ) => {
        event_descriptor!(@build $name, $name, [$($param_name => $param_type),*])
    };

    (
        $name:expr_2021,
        expect = $expect_name:literal,
        [$($param_name:literal => $param_type:ident),* $(,)?]
    ) => {
        event_descriptor!(@build $name, $expect_name, [$($param_name => $param_type),*])
    };

    (
        @build $name:expr_2021,
        $expect_name:literal,
        [$($param_name:literal => $param_type:ident),* $(,)?]
    ) => {
        neo_smart_contract::manifest::ContractEventDescriptor::new(
            ::std::string::String::from($name),
            vec![
                $(
                    neo_smart_contract::manifest::ContractParameterDefinition::new(
                        ::std::string::String::from($param_name),
                        neo_primitives::ContractParameterType::$param_type,
                    )
                    .expect(::std::concat!($expect_name, ".", $param_name))
                ),*
            ],
        )
        .expect(::std::concat!($expect_name, " event descriptor"))
    };
}

pub(crate) use event_descriptor;
