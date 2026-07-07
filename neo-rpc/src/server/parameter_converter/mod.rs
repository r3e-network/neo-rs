//! # neo-rpc::server::parameter_converter
//!
//! RPC parameter parsing and type conversion helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `contract_parameters`: Contract-parameter array conversion.
//! - `domain`: Address, block, and contract identifier RPC conversions.
//! - `scalar`: Scalar and byte-like RPC conversion implementations.
//! - `signers`: RPC signer parameter parsing helpers.
//! - `tokens`: Generic `JToken` shape, numeric, and serde conversion helpers.
//! - `tests`: Module-local tests and regression coverage.

use neo_primitives::UInt160;
use neo_serialization::json::JToken;

use super::model::{Address, SignersAndWitnesses};
use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;

mod contract_parameters;
mod domain;
mod scalar;
mod signers;
#[cfg(test)]
#[path = "../../tests/server/core/parameter_converter.rs"]
mod tests;
mod tokens;

use tokens::{expect_array, expect_object, expect_string, jtoken_to_serde, numeric_from_token};

/// Context supplied when converting RPC parameters.
#[derive(Debug, Clone, Copy)]
pub struct ConversionContext {
    pub address_version: u8,
}

impl ConversionContext {
    pub const fn new(address_version: u8) -> Self {
        Self { address_version }
    }
}

/// Trait implemented by types that can be constructed from a JSON-RPC token.
pub trait RpcConvertible: Sized {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException>;
}

pub struct ParameterConverter;

impl ParameterConverter {
    pub fn convert<T: RpcConvertible>(
        token: &JToken,
        ctx: &ConversionContext,
    ) -> Result<T, RpcException> {
        T::from_token(token, ctx)
    }
}

impl RpcConvertible for SignersAndWitnesses {
    fn from_token(token: &JToken, ctx: &ConversionContext) -> Result<Self, RpcException> {
        signers::parse_signers_and_witnesses(token, ctx)
    }
}

pub(super) fn parse_address(text: &str, address_version: u8) -> Result<Address, RpcException> {
    let mut result = None;
    if UInt160::try_parse(text, &mut result) {
        if let Some(hash) = result {
            return Ok(Address::new(hash, address_version));
        }
    }

    neo_wallets::wallet_helper::WalletAddress::to_script_hash(text, address_version)
        .map(|hash| Address::new(hash, address_version))
        .map_err(|_| invalid_params(format!("Invalid address: {text}")))
}

pub(super) fn parse_uint160(text: &str) -> Result<UInt160, RpcException> {
    let mut result = None;
    if UInt160::try_parse(text, &mut result) {
        if let Some(value) = result {
            return Ok(value);
        }
    }
    Err(invalid_params(format!("Invalid UInt160 value: {text}")))
}

pub(super) fn invalid_params<T: Into<String>>(message: T) -> RpcException {
    RpcException::from(RpcError::invalid_params().with_data(message.into()))
}
