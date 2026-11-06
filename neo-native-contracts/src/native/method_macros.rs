//! Macros for compact native method metadata declarations.
//!
//! Migrated from `neo-core/src/smart_contract/native/method_macros.rs`.
//!
//! # `$crate` path adaptation
//!
//! These macros are defined inside the `neo-native-contracts` crate, so `$crate`
//! resolves to `neo-native-contracts` at every expansion site (all of which live
//! in `crate::native::*` leaves). The original `neo-core` paths are rewritten as
//! follows:
//!
//! | neo-core source path                              | destination path                                            |
//! |---------------------------------------------------|-------------------------------------------------------------|
//! | `$crate::smart_contract::native::NativeMethod`    | `neo_smart_contract::application_engine::NativeMethod`      |
//! | `$crate::smart_contract::native::method_macros::` | `$crate::native::method_macros::`                           |
//! | `$crate::smart_contract::ContractParameterType`   | `neo_primitives::ContractParameterType`                     |
//! | `$crate::smart_contract::call_flags::CallFlags`   | `neo_primitives::CallFlags`                                 |
//! | `$crate::hardfork::Hardfork`                      | `neo_protocol::Hardfork`                                    |
//! | `$crate::smart_contract::ApplicationEngine`       | `neo_smart_contract::application_engine::ApplicationEngine` |
//! | `$crate::error::CoreError` / `CoreResult`         | `neo_smart_contract::error::CoreError` / `CoreResult`       |
//!
//! The self-referential `$crate::native::method_macros::*` paths require task-89's
//! `native/mod.rs` to declare `pub(crate) mod method_macros;` (mirroring the
//! source module root).

macro_rules! neo_native_methods {
    (
        $(
            $kind:tt $name:literal,
            fee = $fee:expr_2021,
            flags = [$($flag:ident),* $(,)?],
            params = [$($param:ident),* $(,)?],
            returns = $return_type:ident
            $(, active = $active:ident)?
            $(, deprecated = $deprecated:ident)?
            $(, storage_fee = $storage_fee:expr_2021)?
            $(, names = [$($param_name:literal),* $(,)?])?
        );+ $(;)?
    ) => {
        vec![
            $(
                neo_smart_contract::application_engine::NativeMethod::new(
                    ::std::string::String::from($name),
                    $fee,
                    $crate::native::method_macros::neo_native_methods!(@is_safe $kind),
                    $crate::native::method_macros::neo_native_methods!(@flags [$($flag),*]),
                    vec![$(neo_primitives::ContractParameterType::$param),*],
                    neo_primitives::ContractParameterType::$return_type,
                )
                $(.with_active_in(neo_protocol::Hardfork::$active))?
                $(.with_deprecated_in(neo_protocol::Hardfork::$deprecated))?
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
        0u8 $(| neo_primitives::CallFlags::$flag.bits())*
    };
}

macro_rules! neo_native_method_metadata {
    (
        ;
        {
            $(
                $kind:tt $name:literal,
                fee = $fee:expr_2021,
                flags = [$($flag:ident),* $(,)?],
                params = [$($param:ident),* $(,)?],
                returns = $return_type:ident
                $(, active = $active:ident)?
                $(, deprecated = $deprecated:ident)?
                $(, storage_fee = $storage_fee:expr_2021)?
                $(, names = [$($param_name:literal),* $(,)?])?
                => $handler_kind:ident $handler:ident
            );+ $(;)?
        }
    ) => {
        $crate::native::method_macros::neo_native_methods![
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
    (@call $contract:expr_2021, $engine:expr_2021, $args:expr_2021, engine, $handler:ident) => {
        $contract.$handler($engine, $args)
    };

    (@call $contract:expr_2021, $engine:expr_2021, $args:expr_2021, engine_only, $handler:ident) => {
        $contract.$handler($engine)
    };

    (@call $contract:expr_2021, $engine:expr_2021, $args:expr_2021, args, $handler:ident) => {
        $contract.$handler($args)
    };

    (
        $contract:expr_2021,
        $engine:expr_2021,
        $method:expr_2021,
        $args:expr_2021,
        aliases = [$($alias:literal => $alias_kind:ident $alias_handler:ident),* $(,)?],
        unknown = $unknown:expr_2021
        ;
        {
            $(
                $kind:tt $name:literal,
                fee = $fee:expr_2021,
                flags = [$($flag:ident),* $(,)?],
                params = [$($param:ident),* $(,)?],
                returns = $return_type:ident
                $(, active = $active:ident)?
                $(, deprecated = $deprecated:ident)?
                $(, storage_fee = $storage_fee:expr_2021)?
                $(, names = [$($param_name:literal),* $(,)?])?
                => $handler_kind:ident $handler:ident
            );+ $(;)?
        }
    ) => {{
        let mut result = None;

        $(
            if result.is_none() && $method == $name {
                result = Some($crate::native::method_macros::neo_native_method_dispatch!(
                    @call $contract, $engine, $args, $handler_kind, $handler
                ));
            }
        )+

        $(
            if result.is_none() && $method == $alias {
                result = Some($crate::native::method_macros::neo_native_method_dispatch!(
                    @call $contract, $engine, $args, $alias_kind, $alias_handler
                ));
            }
        )*

        result.unwrap_or_else(|| Err($unknown($method)))
    }};
}

macro_rules! neo_native_contract_methods {
    ($contract:ty, table = $table:ident $(,)?) => {
        $crate::native::method_macros::neo_native_contract_methods!(
            $contract,
            table = $table,
            aliases = []
        );
    };

    (
        $contract:ty,
        table = $table:ident,
        aliases = [$($alias:literal => $alias_kind:ident $alias_handler:ident),* $(,)?]
        $(,)?
    ) => {
        $crate::native::method_macros::neo_native_contract_methods!(
            $contract,
            table = $table,
            aliases = [$($alias => $alias_kind $alias_handler),*],
            unknown = |method| neo_smart_contract::error::CoreError::native_contract(
                ::std::format!("Unknown method: {}", method)
            )
        );
    };

    (
        $contract:ty,
        table = $table:ident,
        aliases = [$($alias:literal => $alias_kind:ident $alias_handler:ident),* $(,)?],
        unknown = $unknown:expr_2021
        $(,)?
    ) => {
        impl $contract {
            pub(super) fn native_methods() -> ::std::vec::Vec<neo_smart_contract::application_engine::NativeMethod> {
                $table!([$crate::native::method_macros::neo_native_method_metadata];)
            }

            pub(super) fn dispatch_method(
                &self,
                engine: &mut neo_smart_contract::application_engine::ApplicationEngine,
                method: &str,
                args: &[::std::vec::Vec<u8>],
            ) -> neo_smart_contract::error::CoreResult<::std::vec::Vec<u8>> {
                $table!(
                    [$crate::native::method_macros::neo_native_method_dispatch];
                    self,
                    engine,
                    method,
                    args,
                    aliases = [$($alias => $alias_kind $alias_handler),*],
                    unknown = $unknown
                )
            }
        }
    };
}

pub(crate) use neo_native_contract_methods;
pub(crate) use neo_native_method_dispatch;
pub(crate) use neo_native_method_metadata;
pub(crate) use neo_native_methods;
