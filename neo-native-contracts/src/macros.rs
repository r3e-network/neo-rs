//! Local native-contract implementation macros.
//!
//! These macros keep the root of each native contract uniform: handle identity,
//! `NativeContract` identity methods, and binding-table dispatch all follow the
//! same shape across the crate.

macro_rules! native_contract_handle {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            id: $id:expr,
            contract_name: $contract_name:expr,
            hash: $hash:expr $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Default, Clone, Copy)]
        $vis struct $name;

        impl $name {
            #[doc = concat!("Stable native contract id (matches C# `", $contract_name, "`).")]
            pub const ID: i32 = $id;
            #[doc = concat!("Stable native contract name (matches C# `", $contract_name, ".Name`).")]
            pub const NAME: &'static str = $contract_name;

            #[doc = concat!("Construct a new `", stringify!($name), "` handle.")]
            #[must_use]
            pub const fn new() -> Self {
                Self
            }

            #[doc = concat!("Returns the ", $contract_name, " script hash.")]
            #[must_use]
            pub fn hash(&self) -> neo_primitives::UInt160 {
                Self::script_hash()
            }

            #[doc = concat!("Returns the ", $contract_name, " script hash.")]
            #[must_use]
            pub fn script_hash() -> neo_primitives::UInt160 {
                *($hash)
            }
        }
    };
}

macro_rules! native_contract_identity {
    ($contract:ident) => {
        fn id(&self) -> i32 {
            $contract::ID
        }

        fn hash(&self) -> neo_primitives::UInt160 {
            $contract::script_hash()
        }

        fn name(&self) -> &str {
            $contract::NAME
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    };
}

macro_rules! native_contract_dispatch {
    ($bindings:path) => {
        fn invoke(
            &self,
            engine: &mut neo_execution::ApplicationEngine,
            method: &str,
            args: &[Vec<u8>],
        ) -> neo_error::CoreResult<Vec<u8>> {
            crate::support::invoke::dispatch_by_name(self, &$bindings, engine, method, args)
                .unwrap_or_else(|| {
                    Err(neo_error::CoreError::invalid_operation(format!(
                        "{} method '{}({})' is not implemented",
                        self.name(),
                        method,
                        args.len()
                    )))
                })
        }

        fn invoke_resolved(
            &self,
            engine: &mut neo_execution::ApplicationEngine,
            method_index: usize,
            method: &neo_execution::NativeMethod,
            args: &[Vec<u8>],
        ) -> neo_error::CoreResult<Vec<u8>> {
            crate::support::invoke::dispatch_by_index(self, &$bindings, engine, method_index, args)
                .unwrap_or_else(|| {
                    Err(neo_error::CoreError::invalid_operation(format!(
                        "{} method '{}({})' is not implemented",
                        self.name(),
                        method.name,
                        args.len()
                    )))
                })
        }
    };

    ($bindings:path, by_name_and_arity) => {
        fn invoke(
            &self,
            engine: &mut neo_execution::ApplicationEngine,
            method: &str,
            args: &[Vec<u8>],
        ) -> neo_error::CoreResult<Vec<u8>> {
            crate::support::invoke::dispatch_by_name_and_arity(
                self, &$bindings, engine, method, args,
            )
            .unwrap_or_else(|| {
                Err(neo_error::CoreError::invalid_operation(format!(
                    "{} method '{}({})' is not implemented",
                    self.name(),
                    method,
                    args.len()
                )))
            })
        }

        fn invoke_resolved(
            &self,
            engine: &mut neo_execution::ApplicationEngine,
            method_index: usize,
            method: &neo_execution::NativeMethod,
            args: &[Vec<u8>],
        ) -> neo_error::CoreResult<Vec<u8>> {
            crate::support::invoke::dispatch_by_index(self, &$bindings, engine, method_index, args)
                .unwrap_or_else(|| {
                    Err(neo_error::CoreError::invalid_operation(format!(
                        "{} method '{}({})' is not implemented",
                        self.name(),
                        method.name,
                        args.len()
                    )))
                })
        }
    };
}
