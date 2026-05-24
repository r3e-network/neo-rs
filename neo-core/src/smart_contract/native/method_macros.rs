//! Macros for compact native method metadata declarations.

macro_rules! neo_native_methods {
    (
        $(
            $kind:tt $name:literal,
            fee = $fee:expr,
            flags = [$($flag:ident),* $(,)?],
            params = [$($param:ident),* $(,)?],
            returns = $return_type:ident
            $(, active = $active:ident)?
            $(, deprecated = $deprecated:ident)?
            $(, storage_fee = $storage_fee:expr)?
            $(, names = [$($param_name:literal),* $(,)?])?
        );+ $(;)?
    ) => {
        vec![
            $(
                $crate::smart_contract::native::NativeMethod::new(
                    ::std::string::String::from($name),
                    $fee,
                    neo_native_methods!(@is_safe $kind),
                    neo_native_methods!(@flags [$($flag),*]),
                    vec![$($crate::smart_contract::ContractParameterType::$param),*],
                    $crate::smart_contract::ContractParameterType::$return_type,
                )
                $(.with_active_in($crate::hardfork::Hardfork::$active))?
                $(.with_deprecated_in($crate::hardfork::Hardfork::$deprecated))?
                $(.with_storage_fee($storage_fee))?
                $(.with_parameter_names(vec![$(::std::string::String::from($param_name)),*]))?
            ),+
        ]
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
