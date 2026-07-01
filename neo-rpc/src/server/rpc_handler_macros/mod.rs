//! # neo-rpc::server::rpc_handler_macros
//!
//! Macros that bind typed RPC handlers into the registry.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `rpc_handler_macros`: macro helpers for handler registration.

macro_rules! rpc_handlers {
    (protected; $($name:literal => $func:path),+ $(,)?) => {
        vec![
            $($crate::server::rpc_server::protected_rpc_handler($name, $func)),+
        ]
   };
    ($($name:literal => $func:path),+ $(,)?) => {
        vec![
            $($crate::server::rpc_server::rpc_handler($name, $func)),+
        ]
   };
}

pub(crate) use rpc_handlers;
