//! Encrypted transaction submission endpoint (privacy mode, strictly opt-in).
//!
//! The `sendrawtransactionencrypted` method exists only when the node
//! explicitly enables privacy mode and installs an
//! [`EncryptedTransactionSubmitter`] into the RPC server. Without an installed
//! submitter the method is never registered, so the RPC method surface is
//! byte-identical to the pre-privacy behavior.
//!
//! Wire format (mirrors the repo convention of base64 payloads plus hex
//! hashes):
//! - parameter 1: transaction hash (hex, same parsing as other hash params)
//! - parameter 2: `MempoolCiphertext` envelope (base64)
//!
//! The transaction hash is required before decryption because the AEAD layers
//! bind the ciphertext to the hash as associated data, so the caller must
//! commit to the hash up front.
//!
//! On success the response is the relay result plus the serialized enclave
//! attestation report (base64), including the enclave measurements, so the
//! client can verify which enclave code decrypted its transaction.

use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    expect_base64_param_with_decode_message, expect_hash_param, internal_error, invalid_params,
};
use crate::server::rpc_relay;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_primitives::UInt256;
use neo_protocol::payloads::transaction::Transaction;
use serde_json::{Value, json};
use std::sync::Arc;

/// Encrypted mempool transaction envelope (privacy mode).
///
/// This is a stub implementation that accepts the wire format but does not
/// provide actual TEE (Trusted Execution Environment) decryption.
///
/// Production deployment requires:
/// - TEE integration (Intel SGX, AMD SEV, or ARM TrustZone)
/// - AEAD decryption with transaction hash as associated data
/// - Attestation report generation for client verification
/// - Key management and secure enclave initialization
///
/// The stub allows the RPC endpoint to be tested without TEE hardware,
/// enabling development of the surrounding infrastructure (RPC handlers,
/// configuration, client tooling) in parallel with TEE integration.
#[derive(Clone)]
pub struct MempoolCiphertext {
    data: Vec<u8>,
}

impl MempoolCiphertext {
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        Ok(Self {
            data: bytes.to_vec(),
        })
    }

    /// Wraps raw wire bytes (the base64-decoded envelope produced by the
    /// client) into a ciphertext handle.
    ///
    /// The node-side [`EncryptedTransactionSubmitter`] implementation uses
    /// this together with [`MempoolCiphertext::as_bytes`] to hand the envelope
    /// to its own TEE crypto layer for AEAD decryption.
    #[must_use]
    pub fn from_wire_bytes(bytes: Vec<u8>) -> Self {
        Self { data: bytes }
    }

    /// Returns the raw wire bytes of the envelope.
    ///
    /// The bytes are the encoded TEE ciphertext envelope exactly as submitted
    /// by the client; no transformation is applied.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.data
    }
}

/// Host-side decrypting submitter installed by neo-node when privacy mode is
/// explicitly enabled in configuration.
///
/// The implementation is provided by the node (see `neo-node::tee_integration`)
/// and owns the configured key encapsulation plus the enclave attestation
/// service. neo-rpc itself stays free of node-specific wiring.
pub trait EncryptedTransactionSubmitter: Send + Sync {
    /// Decrypts `ciphertext` under the configured key encapsulation, binding it
    /// to the claimed transaction hash, and returns the decoded transaction.
    ///
    /// Fails closed (Err) on any envelope, KEM, AEAD, or transaction decode
    /// problem; never returns partially decrypted data.
    fn open_encrypted(
        &self,
        tx_hash: &UInt256,
        ciphertext: &MempoolCiphertext,
    ) -> Result<Arc<Transaction>, String>;

    /// Serialized enclave attestation report (including measurements) returned
    /// to submitting clients.
    fn attestation_report(&self) -> Result<Vec<u8>, String>;
}

/// Privacy-mode RPC method registration.
pub struct RpcServerPrivacy;

impl RpcServerPrivacy {
    /// Handlers for the opt-in encrypted submission endpoint.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "sendrawtransactionencrypted" => Self::send_raw_transaction_encrypted,
        ]
    }

    fn send_raw_transaction_encrypted(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        const METHOD: &str = "sendrawtransactionencrypted";

        let submitter = server.privacy_submitter().ok_or_else(|| {
            invalid_params("sendrawtransactionencrypted requires privacy mode to be enabled")
        })?;

        let tx_hash = expect_hash_param(params, 0, METHOD)?;
        let raw = expect_base64_param_with_decode_message(
            params,
            1,
            METHOD,
            "Invalid encrypted transaction payload",
        )?;
        let ciphertext = MempoolCiphertext::decode(&raw).map_err(|err| {
            invalid_params(format!("Invalid encrypted transaction payload: {err}"))
        })?;

        let transaction = submitter
            .open_encrypted(&tx_hash, &ciphertext)
            .map_err(|err| invalid_params(format!("Encrypted transaction rejected: {err}")))?;

        let relay_result = rpc_relay::with_relay_responder(server, move |sender| {
            server
                .system()
                .tx_router_actor()
                .try_enqueue_preverify_from((*transaction).clone(), true, Some(sender))
                .map_err(|err| internal_error(err.to_string()))
        })?;
        let mut result = rpc_relay::map_relay_result(relay_result)?;

        let report = submitter
            .attestation_report()
            .map_err(|err| internal_error(format!("Attestation report unavailable: {err}")))?;
        if let Some(object) = result.as_object_mut() {
            object.insert(
                "attestationreport".to_string(),
                json!(BASE64_STANDARD.encode(report)),
            );
        }

        Ok(result)
    }
}
