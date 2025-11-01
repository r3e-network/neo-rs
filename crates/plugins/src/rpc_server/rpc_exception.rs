// Copyright (C) 2015-2025 The Neo Project.
//
// Rust port of Neo.Plugins.RpcServer.RpcException. Represents an exception that
// carries an `RpcError` payload.

use std::fmt::{self, Display, Formatter};

use super::rpc_error::RpcError;

#[derive(Debug, Clone)]
pub struct RpcException {
    error: RpcError,
}

impl RpcException {
    pub fn new(error: RpcError) -> Self {
        Self { error }
    }

    pub fn error(&self) -> &RpcError {
        &self.error
    }
}

impl Display for RpcException {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.error)
    }
}

impl std::error::Error for RpcException {}
