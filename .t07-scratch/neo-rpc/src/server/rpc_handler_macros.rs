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
