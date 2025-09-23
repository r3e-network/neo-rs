//! Rust counterpart to C# RpcException.

pub type RpcException = crate::error::RpcError;
pub type RpcExceptionResult<T> = crate::error::RpcResult<T>;
