//! Typed request parsing for Oracle RPC handlers.
//!
//! Oracle service submission has binary parameters plus a numeric request id.
//! Keeping the JSON-RPC decoding here leaves the handler focused on service
//! lookup and service-error mapping.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{expect_base64_param, expect_u64_param, invalid_params};
use neo_crypto::{ECCurve, ECPoint};
use serde_json::Value;

pub(super) struct SubmitOracleResponseRequest {
    pub(super) oracle_pubkey: ECPoint,
    pub(super) request_id: u64,
    pub(super) tx_signature: Vec<u8>,
    pub(super) message_signature: Vec<u8>,
}

impl SubmitOracleResponseRequest {
    pub(super) fn parse(params: &[Value]) -> Result<Self, RpcException> {
        let oracle_pubkey_bytes = expect_base64_param(params, 0, "submitoracleresponse")?;
        let oracle_pubkey =
            ECPoint::from_bytes_with_curve(ECCurve::Secp256r1, &oracle_pubkey_bytes)
                .map_err(|_| invalid_params("Invalid oracle public key"))?;
        Ok(Self {
            oracle_pubkey,
            request_id: expect_u64_param(params, 1, "submitoracleresponse")?,
            tx_signature: expect_base64_param(params, 2, "submitoracleresponse")?,
            message_signature: expect_base64_param(params, 3, "submitoracleresponse")?,
        })
    }
}
