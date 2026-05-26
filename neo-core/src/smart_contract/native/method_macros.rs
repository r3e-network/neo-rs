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
                    $crate::smart_contract::native::method_macros::neo_native_methods!(@is_safe $kind),
                    $crate::smart_contract::native::method_macros::neo_native_methods!(@flags [$($flag),*]),
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

macro_rules! neo_native_method_metadata {
    (
        ;
        {
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
                => $handler_kind:ident $handler:ident
            );+ $(;)?
        }
    ) => {
        $crate::smart_contract::native::method_macros::neo_native_methods![
            $(
                $kind $name,
                fee = $fee,
                flags = [$($flag),*],
                params = [$($param),*],
                returns = $return_type
                $(, active = $active)?
                $(, deprecated = $deprecated)?
                $(, storage_fee = $storage_fee)?
                $(, names = [$($param_name),*])?
            );+
        ]
    };
}

macro_rules! neo_native_method_dispatch {
    (@call $contract:expr, $engine:expr, $args:expr, engine, $handler:ident) => {
        $contract.$handler($engine, $args)
    };

    (@call $contract:expr, $engine:expr, $args:expr, args, $handler:ident) => {
        $contract.$handler($args)
    };

    (
        $contract:expr,
        $engine:expr,
        $method:expr,
        $args:expr,
        aliases = [$($alias:literal => $alias_kind:ident $alias_handler:ident),* $(,)?],
        unknown = $unknown:expr
        ;
        {
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
                => $handler_kind:ident $handler:ident
            );+ $(;)?
        }
    ) => {{
        let mut result = None;

        $(
            if result.is_none() && $method == $name {
                result = Some($crate::smart_contract::native::method_macros::neo_native_method_dispatch!(
                    @call $contract, $engine, $args, $handler_kind, $handler
                ));
            }
        )+

        $(
            if result.is_none() && $method == $alias {
                result = Some($crate::smart_contract::native::method_macros::neo_native_method_dispatch!(
                    @call $contract, $engine, $args, $alias_kind, $alias_handler
                ));
            }
        )*

        result.unwrap_or_else(|| Err($unknown($method)))
    }};
}

pub(crate) use neo_native_method_dispatch;
pub(crate) use neo_native_method_metadata;
pub(crate) use neo_native_methods;
