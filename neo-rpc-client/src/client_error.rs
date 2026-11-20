//! RPC error types

/// Client-side JSON-RPC error.
///
/// Alias of [`neo_primitives::RpcException`]: the JSON-RPC error payload has a
/// single definition in `neo-primitives`, so the client re-exports it instead of
/// keeping a second struct with the same fields, constructors, and display form.
pub type ClientRpcError = neo_primitives::RpcException;

/// Backwards-compatible alias for the historical client error type name.
pub type RpcException = ClientRpcError;
