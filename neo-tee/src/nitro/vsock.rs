//! Host <-> enclave wire protocol over AF_VSOCK.
//!
//! Defines the request/response message types exchanged between the untrusted
//! host (the `NitroEnclaveSigner`) and the trusted enclave (`EnclaveServer`),
//! a length-framed codec, and a [`VsockTransport`] abstraction with two
//! implementations:
//!
//! * [`MockTransport`] — an in-memory transport driven by a closure, used by
//!   unit tests to exercise the signer and ordering paths with no hardware.
//! * [`RealVsockTransport`] — the AF_VSOCK implementation. Its `connect`/`send`
//!   are clearly flagged EXPERIMENTAL because they require a Nitro environment
//!   (a real `VsockStream` to an enclave CID) to validate.
//!
//! Reference: `claudedocs/aws-hsm-nitro-tee-design.md` §3.2 (`nitro/protocol.rs`,
//! `nitro/transport.rs`).
//!
//! # Framing
//!
//! Each message on the wire is `u32 big-endian length || bincode(payload)`.
//! The length prefix bounds reads so a peer cannot force an unbounded
//! allocation; [`MAX_FRAME_LEN`] caps it.

use crate::error::{TeeError, TeeResult};
use serde::{Deserialize, Serialize};

/// Maximum accepted frame body length (16 MiB). Guards against a malicious or
/// buggy peer advertising an enormous length prefix.
pub const MAX_FRAME_LEN: usize = 16 * 1024 * 1024;

/// Wire protocol version. Bumped on any breaking change to the message types.
pub const PROTOCOL_VERSION: u8 = 1;

/// A request sent from the host into the enclave.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnclaveRequest {
    /// Fetch the enclave's compressed secp256r1 public key and Neo script hash.
    GetPublicKey,
    /// Produce an attestation document, optionally binding `user_data`/`nonce`.
    GetAttestation {
        /// Application bytes to bind into the document.
        user_data: Vec<u8>,
        /// Caller freshness nonce.
        nonce: Vec<u8>,
    },
    /// Sign a block: the enclave SHA-256-hashes `sign_data`, signs with
    /// secp256r1, low-s normalizes, and returns the 64-byte `r||s`.
    SignBlock {
        /// Raw consensus bytes (`network_le(4) || block_hash(32)`), NOT prehashed.
        sign_data: Vec<u8>,
        /// The validator script hash the host expects to sign for.
        script_hash: [u8; 20],
    },
    /// Run the in-enclave fair-ordering sequencer over the supplied tx set and
    /// return an attested ordering proof.
    OrderTxs {
        /// Transactions to order: `(tx_hash, network_fee, system_fee, sender)`.
        txs: Vec<OrderTxEntry>,
        /// Maximum number of ordered hashes to return.
        limit: u32,
    },
}

/// One transaction submitted to [`EnclaveRequest::OrderTxs`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OrderTxEntry {
    /// Transaction hash.
    pub tx_hash: [u8; 32],
    /// Network fee (used by fee-aware ordering policies).
    pub network_fee: i64,
    /// System fee.
    pub system_fee: i64,
    /// Sender script hash.
    pub sender: [u8; 20],
}

/// A response returned by the enclave to the host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EnclaveResponse {
    /// Answer to [`EnclaveRequest::GetPublicKey`].
    PublicKey {
        /// Compressed (33-byte) secp256r1 public key.
        public_key: Vec<u8>,
        /// Neo N3 signature-contract script hash for `public_key`.
        script_hash: [u8; 20],
    },
    /// Answer to [`EnclaveRequest::GetAttestation`]: serialized COSE_Sign1 doc.
    Attestation {
        /// Serialized attestation document bytes.
        document: Vec<u8>,
    },
    /// Answer to [`EnclaveRequest::SignBlock`]: a 64-byte low-s `r||s` signature.
    Signature(Vec<u8>),
    /// Answer to [`EnclaveRequest::OrderTxs`]: the ordering result + proof.
    Ordering(crate::nitro::ordering::OrderingProof),
    /// An error produced while servicing the request.
    Error {
        /// Human-readable error description.
        message: String,
    },
}

/// Encodes a value as a length-framed bincode frame: `u32 BE len || body`.
///
/// # Errors
///
/// Returns [`TeeError::SerializationError`] if bincode encoding fails or the
/// body exceeds [`MAX_FRAME_LEN`].
pub fn encode_frame<T: Serialize>(value: &T) -> TeeResult<Vec<u8>> {
    let body = bincode::serialize(value)
        .map_err(|e| TeeError::SerializationError(format!("frame encode: {e}")))?;
    if body.len() > MAX_FRAME_LEN {
        return Err(TeeError::SerializationError(format!(
            "frame body {} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}",
            body.len()
        )));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_be_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Decodes a length-framed bincode frame produced by [`encode_frame`].
///
/// Returns the decoded value and the total number of bytes consumed (so a
/// caller draining a stream buffer can advance its cursor).
///
/// # Errors
///
/// Returns [`TeeError::SerializationError`] if the buffer is too short, the
/// advertised length exceeds [`MAX_FRAME_LEN`], or bincode decoding fails.
pub fn decode_frame<T: for<'de> Deserialize<'de>>(buf: &[u8]) -> TeeResult<(T, usize)> {
    if buf.len() < 4 {
        return Err(TeeError::SerializationError(
            "frame shorter than 4-byte length prefix".to_string(),
        ));
    }
    let len = u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]) as usize;
    if len > MAX_FRAME_LEN {
        return Err(TeeError::SerializationError(format!(
            "frame length {len} exceeds MAX_FRAME_LEN {MAX_FRAME_LEN}"
        )));
    }
    let end = 4usize
        .checked_add(len)
        .ok_or_else(|| TeeError::SerializationError("frame length overflow".to_string()))?;
    if buf.len() < end {
        return Err(TeeError::SerializationError(format!(
            "frame truncated: have {}, need {end}",
            buf.len()
        )));
    }
    let value = bincode::deserialize(&buf[4..end])
        .map_err(|e| TeeError::SerializationError(format!("frame decode: {e}")))?;
    Ok((value, end))
}

/// Synchronous request/response transport between host and enclave.
///
/// The transport is responsible only for moving framed bytes; the request and
/// response types and the framing are shared by every implementation. Kept
/// synchronous to match the synchronous `ConsensusSigner::sign` seam — a real
/// async vsock implementation drives its own runtime and blocks here.
pub trait VsockTransport: Send + Sync {
    /// Sends a request and waits for the matching response.
    ///
    /// # Errors
    ///
    /// Returns a [`TeeError`] on framing, transport, or remote-side failure.
    fn request(&self, request: &EnclaveRequest) -> TeeResult<EnclaveResponse>;
}

/// In-memory transport for tests.
///
/// Wraps a handler closure that maps a request to a response, exercising the
/// real request/response types and (optionally) the framing codec without any
/// vsock or hardware. Use [`MockTransport::with_framing`] to additionally
/// round-trip every message through [`encode_frame`]/[`decode_frame`], proving
/// the wire codec on the test path.
pub struct MockTransport {
    handler: Box<dyn Fn(&EnclaveRequest) -> TeeResult<EnclaveResponse> + Send + Sync>,
    exercise_framing: bool,
}

impl MockTransport {
    /// Creates a mock transport from a request handler.
    pub fn new<F>(handler: F) -> Self
    where
        F: Fn(&EnclaveRequest) -> TeeResult<EnclaveResponse> + Send + Sync + 'static,
    {
        Self {
            handler: Box::new(handler),
            exercise_framing: false,
        }
    }

    /// Like [`MockTransport::new`] but additionally serializes the request and
    /// deserializes the response through the framing codec, so tests exercise
    /// the on-wire encoding end to end.
    pub fn with_framing<F>(handler: F) -> Self
    where
        F: Fn(&EnclaveRequest) -> TeeResult<EnclaveResponse> + Send + Sync + 'static,
    {
        Self {
            handler: Box::new(handler),
            exercise_framing: true,
        }
    }
}

impl VsockTransport for MockTransport {
    fn request(&self, request: &EnclaveRequest) -> TeeResult<EnclaveResponse> {
        if !self.exercise_framing {
            return (self.handler)(request);
        }

        // Round-trip the request through the codec to mirror the real wire path.
        let req_frame = encode_frame(request)?;
        let (decoded_req, consumed) = decode_frame::<EnclaveRequest>(&req_frame)?;
        debug_assert_eq!(consumed, req_frame.len());

        let response = (self.handler)(&decoded_req)?;

        let resp_frame = encode_frame(&response)?;
        let (decoded_resp, consumed) = decode_frame::<EnclaveResponse>(&resp_frame)?;
        debug_assert_eq!(consumed, resp_frame.len());
        Ok(decoded_resp)
    }
}

/// Address of a Nitro enclave: a vsock context id (CID) and port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VsockAddr {
    /// Enclave context id, read from `nitro-cli describe-enclaves`.
    pub cid: u32,
    /// Listening port inside the enclave.
    pub port: u32,
}

/// AF_VSOCK transport to a running enclave.
///
/// # EXPERIMENTAL: validate in a Nitro environment before production
///
/// Establishing and using a real `VsockStream` requires a Nitro-enabled host
/// and a running enclave bound to `addr.cid:addr.port`. That cannot be
/// exercised off-hardware, so [`RealVsockTransport::request`] is a flagged stub
/// that returns [`TeeError::FeatureNotEnabled`]. A production implementation
/// must: open a `tokio_vsock::VsockStream` to `addr` on its own runtime, write
/// [`encode_frame`] of the request, read the `u32` length prefix then the body,
/// [`decode_frame`] the response, and enforce a bounded timeout so a stalled
/// enclave returns an error (driving a consensus change-view) rather than
/// blocking forever.
pub struct RealVsockTransport {
    addr: VsockAddr,
}

impl RealVsockTransport {
    /// Records the target enclave address.
    ///
    /// # EXPERIMENTAL: validate in a Nitro environment before production
    ///
    /// This does not open a connection; connection management belongs to the
    /// real implementation that must run on Nitro hardware.
    #[must_use]
    pub fn new(addr: VsockAddr) -> Self {
        Self { addr }
    }

    /// Returns the configured enclave address.
    #[must_use]
    pub fn addr(&self) -> VsockAddr {
        self.addr
    }
}

impl VsockTransport for RealVsockTransport {
    fn request(&self, _request: &EnclaveRequest) -> TeeResult<EnclaveResponse> {
        // EXPERIMENTAL: requires a real AF_VSOCK connection to a Nitro enclave.
        // See the type-level doc comment for the implementation contract.
        Err(TeeError::FeatureNotEnabled(format!(
            "RealVsockTransport requires a Nitro environment (cid={}, port={}); \
             not available off-hardware",
            self.addr.cid, self.addr.port
        )))
    }
}

#[cfg(test)]
#[path = "../tests/nitro/vsock.rs"]
mod tests;
