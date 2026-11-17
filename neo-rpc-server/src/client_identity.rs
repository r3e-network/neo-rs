//! Request-scoped client identity for per-client RPC quotas.
//!
//! The transport layer binds the peer identity of the connection serving the
//! current request (see `rpc_server::start_rpc_server`) so handlers that never
//! see the HTTP request - such as the invoke handlers that create iterator
//! sessions - can still attribute resource consumption to a client.

use tokio::task_local;

/// Bucket used when no transport identity is bound (direct handler calls in
/// tests, internal callers). Unattributed callers share one quota so a single
/// hot caller cannot monopolise the global session pool.
pub const ANONYMOUS_CLIENT_KEY: &str = "anonymous";

task_local! {
    static CLIENT_KEY: String;
}

/// Runs `future` with `client_key` bound as the current RPC client identity.
pub async fn scope_client_key<F>(client_key: String, future: F) -> F::Output
where
    F: std::future::Future,
{
    CLIENT_KEY.scope(client_key, future).await
}

/// Returns the client identity bound by the transport, if any.
#[must_use]
pub fn current_client_key() -> Option<String> {
    CLIENT_KEY.try_with(Clone::clone).ok()
}
