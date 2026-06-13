//! Macros for declarative JSON-RPC handler registration.
//!
//! The `rpc_handlers!` macro registers a list of `name → handler` pairs with
//! the RPC server, in either the public or committee-protected handler table.

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
