//! Macros for compact native contract metadata declarations.

macro_rules! event_descriptor {
    (
        $name:literal,
        [$($param_name:literal => $param_type:ident),* $(,)?]
    ) => {
        $crate::smart_contract::manifest::ContractEventDescriptor::new(
            ::std::string::String::from($name),
            vec![
                $(
                    $crate::smart_contract::manifest::ContractParameterDefinition::new(
                        ::std::string::String::from($param_name),
                        $crate::smart_contract::ContractParameterType::$param_type,
                    )
                    .expect(::std::concat!($name, ".", $param_name))
                ),*
            ],
        )
        .expect(::std::concat!($name, " event descriptor"))
    };
}

pub(crate) use event_descriptor;
