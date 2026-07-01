//! # neo-rpc::server::dispatch
//!
//! RPC method dispatch, registration, and handler lookup helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `dispatch`: RPC dispatch table and handler invocation helpers.

use super::rpc_error::RpcError;
use super::rpc_remote_ledger::should_proxy_remote_ledger_method;
use super::rpc_server::{RpcHandler, RpcServer};
use super::rpc_server_settings::{RpcServerSettings, UnhandledExceptionPolicy};
use parking_lot::RwLock;
use std::collections::HashSet;
use std::panic::{self, AssertUnwindSafe};
use std::sync::{Arc, Weak};
use tracing::error;

pub struct Dispatch;

impl Dispatch {
    /// Look up a registered RPC handler by method name (case-insensitive).
    ///
    /// Returns `Err(RpcError::access_denied())` for disabled methods,
    /// `Err(RpcError::internal_server_error())` if the server has been
    /// dropped, and `Err(RpcError::method_not_found())` for unknown methods.
    pub(crate) fn resolve_rpc_handler(
        server: &Weak<RwLock<RpcServer>>,
        disabled: &HashSet<String>,
        method: &str,
    ) -> Result<(Arc<RwLock<RpcServer>>, Arc<RpcHandler>), RpcError> {
        let method_key = method.to_ascii_lowercase();
        if disabled.contains(&method_key) {
            return Err(RpcError::access_denied());
        }

        let Some(server_arc) = server.upgrade() else {
            return Err(RpcError::internal_server_error());
        };

        let Some(handler) = Dispatch::lookup_rpc_handler(&server_arc, &method_key) else {
            return Err(RpcError::method_not_found().with_data(method));
        };

        Ok((server_arc, handler))
    }

    /// Look up a handler in the server's method registry.
    pub(crate) fn lookup_rpc_handler(
        server_arc: &Arc<RwLock<RpcServer>>,
        method_key: &str,
    ) -> Option<Arc<RpcHandler>> {
        let server_guard = server_arc.read();
        let guard = server_guard.handlers_guard();
        guard.get(method_key).cloned()
    }

    /// Invoke a registered handler, catching panics and applying the
    /// configured `UnhandledExceptionPolicy`.
    pub(crate) fn invoke_rpc_handler(
        server_arc: &Arc<RwLock<RpcServer>>,
        handler: Arc<RpcHandler>,
        method: &str,
        params: &[serde_json::Value],
    ) -> Result<serde_json::Value, RpcError> {
        let policy = RpcServerSettings::current().exception_policy();
        let callback = handler.callback();
        let canonical_method = handler.descriptor().name.clone();
        let remote_ledger = {
            let server_guard = server_arc.read();
            server_guard.check_rate_limit(&canonical_method)?;
            if should_proxy_remote_ledger_method(&canonical_method) {
                server_guard.remote_ledger_rpc().cloned()
            } else {
                None
            }
        };
        if let Some(remote) = remote_ledger {
            return remote.call(&canonical_method, params);
        }
        let call_result = panic::catch_unwind(AssertUnwindSafe(|| {
            let server_guard = server_arc.read();
            (callback)(&server_guard, params)
        }));

        match call_result {
            Ok(Ok(result)) => Ok(result),
            Ok(Err(err)) => Err(RpcError::from(err)),
            Err(payload) => {
                error!(
                    target: "neo::rpc",
                    method,
                    error = panic_message(&payload),
                    "rpc handler panicked"
                );
                match policy {
                    UnhandledExceptionPolicy::StopPlugin => {
                        let mut server = server_arc.write();
                        server.stop_rpc_server();
                    }
                    UnhandledExceptionPolicy::StopNode => std::process::exit(1),
                    UnhandledExceptionPolicy::Terminate => std::process::abort(),
                    UnhandledExceptionPolicy::Ignore
                    | UnhandledExceptionPolicy::Log
                    | UnhandledExceptionPolicy::Continue => {}
                }
                Err(RpcError::internal_server_error())
            }
        }
    }
}

fn panic_message(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(message) = payload.downcast_ref::<&str>() {
        (*message).to_string()
    } else if let Some(message) = payload.downcast_ref::<String>() {
        message.clone()
    } else {
        "panic".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::rpc_server::{RpcHandler, RpcServer};
    use crate::server::rpc_server_settings::RpcServerConfig;
    use crate::server::test_support::test_system;
    use serde_json::{Value, json};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn serve_rpc_once(expected_method: &'static str, result: Value) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test RPC");
        let url = format!("http://{}", listener.local_addr().expect("addr"));
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().expect("accept");
            let mut request = Vec::new();
            let mut buf = [0u8; 4096];
            loop {
                let read = stream.read(&mut buf).expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buf[..read]);
                if request.windows(4).any(|window| window == b"\r\n\r\n") {
                    break;
                }
            }
            let text = String::from_utf8_lossy(&request);
            assert!(
                text.contains(&format!(r#""method":"{expected_method}""#))
                    || text.contains(&format!(r#""method": "{expected_method}""#)),
                "unexpected request: {text}"
            );
            let body = json!({
                "jsonrpc": "2.0",
                "id": 1,
                "result": result,
            })
            .to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream
                .write_all(response.as_bytes())
                .expect("write response");
        });
        url
    }

    fn test_server_with_handler(method: &'static str) -> Arc<RwLock<RpcServer>> {
        let system = test_system(neo_config::ProtocolSettings::default());
        let mut server = RpcServer::new(system, RpcServerConfig::default());
        server.register_method(RpcHandler::new(
            crate::server::RpcMethodDescriptor::new(method),
            Arc::new(|_, _| Ok(Value::String("local".to_string()))),
        ));
        Arc::new(RwLock::new(server))
    }

    fn assert_method_is_proxied(method: &'static str, params: &[Value]) {
        let server = test_server_with_handler(method);
        server
            .write()
            .set_remote_ledger_rpc(serve_rpc_once(method, json!({"proxied": method})))
            .expect("configure remote ledger");
        let handler = Dispatch::lookup_rpc_handler(&server, method).expect("handler");

        let result =
            Dispatch::invoke_rpc_handler(&server, handler, method, params).expect("invoke");

        assert_eq!(result, json!({"proxied": method}));
    }

    #[test]
    fn remote_ledger_dispatch_proxies_read_only_invocations() {
        let server = test_server_with_handler("invokefunction");
        server
            .write()
            .set_remote_ledger_rpc(serve_rpc_once("invokefunction", json!("remote")))
            .expect("configure remote ledger");
        let handler = Dispatch::lookup_rpc_handler(&server, "invokefunction").expect("handler");

        let result =
            Dispatch::invoke_rpc_handler(&server, handler, "invokefunction", &[]).expect("invoke");

        assert_eq!(result, json!("remote"));
    }

    #[test]
    fn remote_ledger_dispatch_proxies_indexer_reads() {
        let server = test_server_with_handler("getblocktransactions");
        server
            .write()
            .set_remote_ledger_rpc(serve_rpc_once(
                "getblocktransactions",
                json!([{"hash": "remote"}]),
            ))
            .expect("configure remote ledger");
        let handler =
            Dispatch::lookup_rpc_handler(&server, "getblocktransactions").expect("handler");

        let result =
            Dispatch::invoke_rpc_handler(&server, handler, "getblocktransactions", &[json!(1)])
                .expect("invoke");

        assert_eq!(result, json!([{"hash": "remote"}]));
    }

    #[test]
    fn remote_ledger_dispatch_proxies_state_service_reads() {
        assert_method_is_proxied("getstateheight", &[]);
        assert_method_is_proxied("getstateroot", &[json!(123)]);
        assert_method_is_proxied("getproof", &[json!("0x00"), json!("0x00"), json!("AA==")]);
        assert_method_is_proxied("getstate", &[json!("0x00"), json!("0x00"), json!("AA==")]);
        assert_method_is_proxied("findstates", &[json!("0x00"), json!("0x00"), json!("AA==")]);
        assert_method_is_proxied("verifyproof", &[json!("0x00"), json!("AA==")]);
    }

    #[test]
    fn remote_ledger_proxy_policy_covers_every_transport_method() {
        let system = test_system(neo_config::ProtocolSettings::default());
        let config = RpcServerConfig {
            rpc_user: "neo".to_string(),
            rpc_pass: "password".to_string(),
            ..RpcServerConfig::default()
        };
        let mut server = RpcServer::new(system, config);
        server.register_handlers(crate::server::RpcServerBlockchain::register_handlers());
        server.register_handlers(crate::server::RpcServerNode::register_handlers());
        server.register_handlers(crate::server::RpcServerState::register_handlers());
        server.register_handlers(crate::server::RpcServerWallet::register_handlers());
        server.register_handlers(crate::server::RpcServerUtilities::register_handlers());
        server.register_handlers(crate::server::RpcServerSmartContract::register_handlers());
        server.register_handlers(crate::server::RpcServerApplicationLogs::register_handlers());
        server.register_handlers(crate::server::RpcServerTokensTracker::register_handlers());
        server.register_handlers(crate::server::RpcServerIndexer::register_handlers());
        server.register_handlers(crate::server::RpcServerOracle::register_handlers());

        let intentionally_local = [
            "closewallet",
            "dumpprivkey",
            "getconnectioncount",
            "getnewaddress",
            "getpeers",
            "importprivkey",
            "listaddress",
            "openwallet",
            "validateaddress",
        ];
        let hybrid_remote_ledger = [
            // getversion describes this process (tcpport/nonce/useragent), but
            // remote-ledger mode sources its dynamic protocol policy fields
            // from the upstream ledger's getversion response.
            "getversion",
        ];
        let missing_policy = server
            .transport_method_names()
            .into_iter()
            .filter(|method| {
                !should_proxy_remote_ledger_method(method)
                    && !intentionally_local.contains(&method.as_str())
                    && !hybrid_remote_ledger.contains(&method.as_str())
            })
            .collect::<Vec<_>>();

        assert!(
            missing_policy.is_empty(),
            "remote-ledger mode must proxy ledger-derived RPC methods or explicitly classify them as process-local: {missing_policy:?}"
        );
    }

    #[test]
    fn remote_ledger_dispatch_proxies_with_canonical_method_name() {
        let server = test_server_with_handler("getblockcount");
        server
            .write()
            .set_remote_ledger_rpc(serve_rpc_once("getblockcount", json!(123)))
            .expect("configure remote ledger");
        let handler = Dispatch::lookup_rpc_handler(&server, "getblockcount").expect("handler");

        let result =
            Dispatch::invoke_rpc_handler(&server, handler, "GetBlockCount", &[]).expect("invoke");

        assert_eq!(result, json!(123));
    }

    #[test]
    fn remote_ledger_dispatch_proxies_mempool_and_relay_methods() {
        let server = test_server_with_handler("sendrawtransaction");
        server
            .write()
            .set_remote_ledger_rpc(serve_rpc_once(
                "sendrawtransaction",
                json!({"hash": "0xremote"}),
            ))
            .expect("configure remote ledger");
        let handler = Dispatch::lookup_rpc_handler(&server, "sendrawtransaction").expect("handler");

        let result =
            Dispatch::invoke_rpc_handler(&server, handler, "sendrawtransaction", &[json!("AA==")])
                .expect("invoke");

        assert_eq!(result, json!({"hash": "0xremote"}));
    }

    #[test]
    fn remote_ledger_dispatch_proxies_wallet_transaction_methods() {
        for method in ["sendfrom", "sendtoaddress", "sendmany", "canceltransaction"] {
            assert_method_is_proxied(method, &[json!("placeholder")]);
        }
    }

    #[test]
    fn remote_ledger_dispatch_proxies_oracle_submission() {
        assert_method_is_proxied("submitoracleresponse", &[json!("placeholder")]);
    }

    #[test]
    fn remote_ledger_dispatch_proxies_iterator_sessions() {
        let server = test_server_with_handler("traverseiterator");
        server
            .write()
            .set_remote_ledger_rpc(serve_rpc_once("traverseiterator", json!(["remote-item"])))
            .expect("configure remote ledger");
        let handler = Dispatch::lookup_rpc_handler(&server, "traverseiterator").expect("handler");

        let result = Dispatch::invoke_rpc_handler(&server, handler, "traverseiterator", &[])
            .expect("invoke");

        assert_eq!(result, json!(["remote-item"]));
    }

    #[test]
    fn remote_ledger_dispatch_proxies_service_inventory() {
        assert_method_is_proxied("listplugins", &[]);
        assert_method_is_proxied("listservices", &[]);
    }

    #[test]
    fn remote_ledger_dispatch_keeps_local_process_methods_local() {
        let server = test_server_with_handler("getversion");
        server
            .write()
            .set_remote_ledger_rpc("http://127.0.0.1:9")
            .expect("configure remote ledger");
        let handler = Dispatch::lookup_rpc_handler(&server, "getversion").expect("handler");

        let result =
            Dispatch::invoke_rpc_handler(&server, handler, "getversion", &[]).expect("invoke");

        assert_eq!(result, json!("local"));
    }
}
