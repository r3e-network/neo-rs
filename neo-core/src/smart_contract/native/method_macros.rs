//! Macros for compact native method metadata declarations.

macro_rules! neo_native_methods {
    (
        $(
            $kind:tt $name:literal,
            fee = $fee:expr,
            flags = [$($flag:ident),* $(,)?],
            params = [$($param:ident),* $(,)?],
            returns = $return_type:ident
            $(, names = [$($param_name:literal),* $(,)?])?
        );+ $(;)?
    ) => {
        vec![
            $(
                neo_native_methods!(
                    @method
                    $kind,
                    $name,
                    $fee,
                    [$($flag),*],
                    [$($param),*],
                    $return_type
                    $(, names = [$($param_name),*])?
                )
            ),+
        ]
    };

    (
        @method
        $kind:tt,
        $name:literal,
        $fee:expr,
        [$($flag:ident),*],
        [$($param:ident),*],
        $return_type:ident,
        names = [$($param_name:literal),*]
    ) => {{
        neo_native_methods!(
            @base $kind, $name, $fee, [$($flag),*], [$($param),*], $return_type
        )
        .with_parameter_names(vec![$(String::from($param_name)),*])
    }};

    (
        @method
        $kind:tt,
        $name:literal,
        $fee:expr,
        [$($flag:ident),*],
        [$($param:ident),*],
        $return_type:ident
    ) => {{
        neo_native_methods!(
            @base $kind, $name, $fee, [$($flag),*], [$($param),*], $return_type
        )
    }};

    (
        @base
        $kind:tt,
        $name:literal,
        $fee:expr,
        [$($flag:ident),*],
        [$($param:ident),*],
        $return_type:ident
    ) => {
        $crate::smart_contract::native::NativeMethod::new(
            String::from($name),
            $fee,
            neo_native_methods!(@is_safe $kind),
            neo_native_methods!(@flags [$($flag),*]),
            vec![$($crate::smart_contract::ContractParameterType::$param),*],
            $crate::smart_contract::ContractParameterType::$return_type,
        )
    };

    (@is_safe safe) => {
        true
    };

    (@is_safe unsafe) => {
        false
    };

    (@flags [$($flag:ident),*]) => {
        0u8 $(| $crate::smart_contract::call_flags::CallFlags::$flag.bits())*
    };
}

pub(crate) use neo_native_methods;
