// Copyright (C) 2015-2025 The Neo Project.
//
// Rust equivalent of Neo.Plugins.RpcServer.Result — helper routines for working
// with `RpcError`/`RpcException` in a fluent manner.

use std::error::Error;

use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;

/// Executes a function and ensures the outcome is `Some`, otherwise yields an
/// `RpcException` built from `err`. Any error produced by the function is
/// converted into an `RpcException`, optionally appending the underlying error
/// message when `with_data` is true.
pub fn ok_or<T, F, E>(function: F, err: RpcError, with_data: bool) -> Result<T, RpcException>
where
    F: FnOnce() -> Result<Option<T>, E>,
    E: Error + 'static,
{
    match function() {
        Ok(Some(value)) => Ok(value),
        Ok(None) => Err(RpcException::from(err)),
        Err(e) => {
            if with_data {
                Err(RpcException::from(err.with_data(e.to_string())))
            } else {
                Err(RpcException::from(err))
            }
        }
    }
}

/// Executes a function returning a boolean, ensuring the result is `true`.
pub fn true_or<F, E>(function: F, err: RpcError) -> Result<bool, RpcException>
where
    F: FnOnce() -> Result<bool, E>,
    E: Error + 'static,
{
    match function() {
        Ok(result) if result => Ok(true),
        Ok(_) | Err(_) => Err(RpcException::from(err)),
    }
}

/// Extension utilities for `Option<T>` mirroring the C# helpers.
pub trait OptionRpcExt<T> {
    fn not_null_or(self, err: RpcError) -> Result<T, RpcException>;
    fn null_or(self, err: RpcError) -> Result<(), RpcException>;
}

impl<T> OptionRpcExt<T> for Option<T> {
    fn not_null_or(self, err: RpcError) -> Result<T, RpcException> {
        self.ok_or_else(|| RpcException::from(err))
    }

    fn null_or(self, err: RpcError) -> Result<(), RpcException> {
        if self.is_none() {
            Ok(())
        } else {
            Err(RpcException::from(err))
        }
    }
}

/// Extension helpers for booleans to enforce expected results.
pub trait BoolRpcExt {
    fn true_or(self, err: RpcError) -> Result<bool, RpcException>;
    fn false_or(self, err: RpcError) -> Result<bool, RpcException>;
}

impl BoolRpcExt for bool {
    fn true_or(self, err: RpcError) -> Result<bool, RpcException> {
        if self {
            Ok(true)
        } else {
            Err(RpcException::from(err))
        }
    }

    fn false_or(self, err: RpcError) -> Result<bool, RpcException> {
        if !self {
            Ok(false)
        } else {
            Err(RpcException::from(err))
        }
    }
}
