//! AWS Nitro NSM attestation-document model and a pure parser.
//!
//! A Nitro attestation document is a [COSE_Sign1] envelope (RFC 8152) whose
//! payload is a CBOR map produced by the Nitro Secure Module (NSM). This module
//! provides:
//!
//! * [`NitroAttestationDoc`] — the parsed document model.
//! * [`parse_cose_sign1`] — a PURE parser for the COSE_Sign1 envelope.
//! * [`NitroAttestationDoc::parse_payload`] — a PURE parser + structural
//!   validator for the CBOR payload (the fields NSM defines).
//! * [`NitroAttestationDoc::structural_validate`] — checks the field invariants
//!   AWS documents (module_id non-empty, digest == "SHA384", PCR map shape,
//!   certificate present, etc.).
//! * [`verify_pki_chain`] — a clearly-flagged STUB for full PKI-chain
//!   verification against the pinned Nitro Root G1.
//!
//! Reference: `claudedocs/aws-hsm-nitro-tee-design.md` §3.2, §7.
//!
//! [COSE_Sign1]: https://www.rfc-editor.org/rfc/rfc8152#section-4.2
//!
//! # Trust model (read before relying on this)
//!
//! Parsing and structural validation here are real and fully tested. They are
//! **necessary but not sufficient** for trust. The complete trust decision
//! additionally requires, all of which are flagged EXPERIMENTAL below:
//!
//! 1. COSE_Sign1 ES384 signature verification over the protected header +
//!    payload using the document's leaf certificate public key.
//! 2. X.509 chain verification from the leaf, through `cabundle`, to the pinned
//!    AWS Nitro Root G1 certificate (validity windows + key usage + the pinned
//!    root fingerprint).
//! 3. PCR pinning (PCR0/1/2/8 equal to operator-expected values) and rejection
//!    of all-zero PCRs (which a `--debug-mode` enclave emits).
//! 4. Freshness (timestamp within an acceptable window) and single-use nonce.

use crate::error::{TeeError, TeeResult};
use std::collections::BTreeMap;

/// SHA-256 fingerprint of the AWS Nitro Enclaves Attestation PKI Root G1.
///
/// This is the pin a full verifier compares the chain's trust anchor against.
/// Source: `claudedocs/aws-hsm-nitro-tee-design.md` §6 / AWS published value.
pub const NITRO_ROOT_G1_SHA256_FINGERPRINT: [u8; 32] = [
    0x64, 0x1A, 0x03, 0x21, 0xA3, 0xE2, 0x44, 0xEF, 0xE4, 0x56, 0x46, 0x31, 0x95, 0xD6, 0x06, 0x31,
    0x7E, 0xD7, 0xCD, 0xCC, 0x3C, 0x17, 0x56, 0xE0, 0x98, 0x93, 0xF3, 0xC6, 0x8F, 0x79, 0xBB, 0x5B,
];

/// The digest algorithm NSM uses for PCRs and the document digest field.
pub const NITRO_DIGEST_ALGORITHM: &str = "SHA384";

/// Length in bytes of a Nitro PCR value (SHA-384 = 48 bytes).
pub const NITRO_PCR_LEN: usize = 48;

/// Maximum number of PCR registers NSM exposes (PCR0..=PCR31, but only a small
/// subset are meaningful). Used as a sanity bound during parsing.
pub const NITRO_MAX_PCRS: usize = 32;

/// A parsed Nitro NSM attestation document payload.
///
/// Field names mirror the NSM CBOR map keys defined by AWS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NitroAttestationDoc {
    /// Identifier of the enclave's NSM module instance.
    pub module_id: String,
    /// Digest algorithm string (must be `"SHA384"` for a genuine document).
    pub digest: String,
    /// Milliseconds since the Unix epoch at which NSM produced the document.
    pub timestamp_ms: u64,
    /// Platform Configuration Registers, keyed by index. Each value is 48 bytes
    /// (SHA-384). PCR0/1/2 measure the EIF; PCR8 measures the signing cert.
    pub pcrs: BTreeMap<u8, Vec<u8>>,
    /// DER-encoded leaf certificate that signed this document (the NSM key).
    pub certificate: Vec<u8>,
    /// Intermediate CA certificates from the leaf up toward (but excluding) the
    /// pinned Root G1, in order. DER-encoded.
    pub cabundle: Vec<Vec<u8>>,
    /// Optional application-bound public key (e.g. an ephemeral RSA key for the
    /// KMS attested-decrypt import path).
    pub public_key: Option<Vec<u8>>,
    /// Optional application-bound user data (e.g. a hash of an ordering proof).
    pub user_data: Option<Vec<u8>>,
    /// Optional caller-supplied nonce for challenge-response freshness.
    pub nonce: Option<Vec<u8>>,
}

/// Result of verifying the PKI chain + COSE signature of a document.
///
/// Produced by [`verify_pki_chain`]. The `Stub` variant is what the current
/// experimental implementation returns; a real verifier returns `Verified`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PkiVerification {
    /// The chain verified to the pinned Nitro Root G1 and the COSE signature is
    /// valid. Only a real implementation may return this.
    Verified,
    /// Verification was skipped because the full implementation is not present.
    /// Callers MUST treat this as "untrusted" outside of tests/simulation.
    Stub,
}

/// Options controlling structural + freshness validation of a document.
#[derive(Debug, Clone)]
pub struct NitroValidationOptions {
    /// Expected PCR0 (EIF image measurement). `None` skips this pin.
    pub expected_pcr0: Option<[u8; NITRO_PCR_LEN]>,
    /// Expected PCR1 (Linux kernel + bootstrap). `None` skips this pin.
    pub expected_pcr1: Option<[u8; NITRO_PCR_LEN]>,
    /// Expected PCR2 (application). `None` skips this pin.
    pub expected_pcr2: Option<[u8; NITRO_PCR_LEN]>,
    /// Expected PCR8 (signing certificate; only present for a signed EIF).
    pub expected_pcr8: Option<[u8; NITRO_PCR_LEN]>,
    /// Reject documents whose PCR0/1/2 are all-zero (a `--debug-mode` enclave).
    pub reject_zero_pcrs: bool,
}

impl Default for NitroValidationOptions {
    fn default() -> Self {
        Self {
            expected_pcr0: None,
            expected_pcr1: None,
            expected_pcr2: None,
            expected_pcr8: None,
            reject_zero_pcrs: true,
        }
    }
}

/// A parsed COSE_Sign1 envelope (RFC 8152 §4.2): a 4-element CBOR array of
/// `[protected, unprotected, payload, signature]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoseSign1 {
    /// `protected` header as a serialized CBOR byte string (signed verbatim).
    pub protected: Vec<u8>,
    /// `payload` byte string — the CBOR-encoded NSM document map.
    pub payload: Vec<u8>,
    /// `signature` byte string — ES384 over the COSE `Sig_structure`.
    pub signature: Vec<u8>,
}

/// Parses a COSE_Sign1 envelope from its CBOR encoding.
///
/// This is a PURE function: it does no signature verification, it only decodes
/// the 4-element array structure and extracts the byte fields. The
/// `unprotected` header is intentionally not retained (NSM does not place
/// security-relevant fields there).
///
/// # Errors
///
/// Returns [`TeeError::InvalidAttestationReport`] if the bytes are not a
/// 4-element CBOR array of the expected shape.
pub fn parse_cose_sign1(bytes: &[u8]) -> TeeResult<CoseSign1> {
    let value: ciborium::value::Value = ciborium::de::from_reader(bytes)
        .map_err(|e| TeeError::InvalidAttestationReport(format!("COSE_Sign1 CBOR decode: {e}")))?;

    let array = value.as_array().ok_or_else(|| {
        TeeError::InvalidAttestationReport("COSE_Sign1 is not a CBOR array".to_string())
    })?;

    if array.len() != 4 {
        return Err(TeeError::InvalidAttestationReport(format!(
            "COSE_Sign1 must have 4 elements, got {}",
            array.len()
        )));
    }

    let protected = cbor_bytes(&array[0], "COSE protected header")?;
    // array[1] is the unprotected header map — not retained.
    let payload = cbor_bytes(&array[2], "COSE payload")?;
    let signature = cbor_bytes(&array[3], "COSE signature")?;

    Ok(CoseSign1 {
        protected,
        payload,
        signature,
    })
}

impl NitroAttestationDoc {
    /// Parses a full attestation document (COSE_Sign1 envelope -> payload).
    ///
    /// This decodes the envelope with [`parse_cose_sign1`], then the inner CBOR
    /// payload with [`NitroAttestationDoc::parse_payload`]. It performs NO
    /// signature or PKI verification — see [`verify_pki_chain`].
    ///
    /// # Errors
    ///
    /// Propagates parse errors from either stage.
    pub fn parse(document: &[u8]) -> TeeResult<Self> {
        let envelope = parse_cose_sign1(document)?;
        Self::parse_payload(&envelope.payload)
    }

    /// Parses the CBOR payload map of an attestation document.
    ///
    /// PURE function. Decodes the NSM-defined fields and applies basic type
    /// checks, but does NOT enforce the semantic invariants — call
    /// [`NitroAttestationDoc::structural_validate`] for those.
    ///
    /// # Errors
    ///
    /// Returns [`TeeError::InvalidAttestationReport`] on a malformed map.
    pub fn parse_payload(payload: &[u8]) -> TeeResult<Self> {
        let value: ciborium::value::Value = ciborium::de::from_reader(payload)
            .map_err(|e| TeeError::InvalidAttestationReport(format!("payload CBOR decode: {e}")))?;

        let map = value.as_map().ok_or_else(|| {
            TeeError::InvalidAttestationReport("payload is not a CBOR map".to_string())
        })?;

        let mut module_id: Option<String> = None;
        let mut digest: Option<String> = None;
        let mut timestamp_ms: Option<u64> = None;
        let mut pcrs: BTreeMap<u8, Vec<u8>> = BTreeMap::new();
        let mut certificate: Option<Vec<u8>> = None;
        let mut cabundle: Vec<Vec<u8>> = Vec::new();
        let mut public_key: Option<Vec<u8>> = None;
        let mut user_data: Option<Vec<u8>> = None;
        let mut nonce: Option<Vec<u8>> = None;

        for (key, val) in map {
            let Some(field) = key.as_text() else {
                // NSM keys are all text strings; ignore unexpected key types.
                continue;
            };
            match field {
                "module_id" => {
                    module_id = Some(text_field(val, "module_id")?);
                }
                "digest" => {
                    digest = Some(text_field(val, "digest")?);
                }
                "timestamp" => {
                    timestamp_ms = Some(uint_field(val, "timestamp")?);
                }
                "pcrs" => {
                    pcrs = parse_pcr_map(val)?;
                }
                "certificate" => {
                    certificate = Some(cbor_bytes(val, "certificate")?);
                }
                "cabundle" => {
                    cabundle = parse_cert_array(val)?;
                }
                "public_key" => {
                    public_key = optional_bytes(val);
                }
                "user_data" => {
                    user_data = optional_bytes(val);
                }
                "nonce" => {
                    nonce = optional_bytes(val);
                }
                _ => {}
            }
        }

        Ok(Self {
            module_id: module_id.ok_or_else(|| missing("module_id"))?,
            digest: digest.ok_or_else(|| missing("digest"))?,
            timestamp_ms: timestamp_ms.ok_or_else(|| missing("timestamp"))?,
            pcrs,
            certificate: certificate.ok_or_else(|| missing("certificate"))?,
            cabundle,
            public_key,
            user_data,
            nonce,
        })
    }

    /// Validates the structural invariants AWS documents for an NSM document.
    ///
    /// This is real, tested validation. It checks: non-empty `module_id`; the
    /// digest algorithm is `"SHA384"`; PCR indices are in range and each PCR is
    /// 48 bytes; the certificate is non-empty; and (when requested) that
    /// PCR0/1/2 are not all-zero. When `expected_pcrN` pins are provided, the
    /// corresponding PCR must be present and equal.
    ///
    /// It does NOT verify the COSE signature or the certificate chain — that is
    /// the responsibility of [`verify_pki_chain`].
    ///
    /// # Errors
    ///
    /// Returns [`TeeError::InvalidAttestationReport`] describing the first
    /// violated invariant.
    pub fn structural_validate(&self, options: &NitroValidationOptions) -> TeeResult<()> {
        if self.module_id.is_empty() {
            return Err(TeeError::InvalidAttestationReport(
                "module_id is empty".to_string(),
            ));
        }

        if self.digest != NITRO_DIGEST_ALGORITHM {
            return Err(TeeError::InvalidAttestationReport(format!(
                "unexpected digest algorithm: {} (want {NITRO_DIGEST_ALGORITHM})",
                self.digest
            )));
        }

        if self.certificate.is_empty() {
            return Err(TeeError::InvalidAttestationReport(
                "certificate is empty".to_string(),
            ));
        }

        if self.pcrs.is_empty() {
            return Err(TeeError::InvalidAttestationReport(
                "no PCRs present".to_string(),
            ));
        }

        for (index, pcr) in &self.pcrs {
            if (*index as usize) >= NITRO_MAX_PCRS {
                return Err(TeeError::InvalidAttestationReport(format!(
                    "PCR index {index} out of range"
                )));
            }
            if pcr.len() != NITRO_PCR_LEN {
                return Err(TeeError::InvalidAttestationReport(format!(
                    "PCR{index} has length {} (want {NITRO_PCR_LEN})",
                    pcr.len()
                )));
            }
        }

        if options.reject_zero_pcrs {
            // A genuine production enclave never emits all-zero PCR0/1/2;
            // `--debug-mode` enclaves do. Fail closed.
            for index in [0u8, 1, 2] {
                if let Some(pcr) = self.pcrs.get(&index) {
                    if pcr.iter().all(|b| *b == 0) {
                        return Err(TeeError::InvalidAttestationReport(format!(
                            "PCR{index} is all-zero (debug-mode enclave?) — rejected"
                        )));
                    }
                }
            }
        }

        self.check_pcr_pin(0, options.expected_pcr0.as_ref())?;
        self.check_pcr_pin(1, options.expected_pcr1.as_ref())?;
        self.check_pcr_pin(2, options.expected_pcr2.as_ref())?;
        self.check_pcr_pin(8, options.expected_pcr8.as_ref())?;

        Ok(())
    }

    /// Returns the PCR value at `index`, if present.
    #[must_use]
    pub fn pcr(&self, index: u8) -> Option<&[u8]> {
        self.pcrs.get(&index).map(Vec::as_slice)
    }

    fn check_pcr_pin(&self, index: u8, expected: Option<&[u8; NITRO_PCR_LEN]>) -> TeeResult<()> {
        let Some(expected) = expected else {
            return Ok(());
        };
        let actual = self.pcrs.get(&index).ok_or_else(|| {
            TeeError::InvalidAttestationReport(format!("PCR{index} pin set but PCR absent"))
        })?;
        if actual.as_slice() != expected.as_slice() {
            return Err(TeeError::InvalidAttestationReport(format!(
                "PCR{index} mismatch"
            )));
        }
        Ok(())
    }
}

/// Verifies the COSE_Sign1 signature and X.509 chain of an attestation document
/// against the pinned Nitro Root G1.
///
/// # EXPERIMENTAL: validate in a Nitro environment before production
///
/// This is a STUB. A production implementation MUST:
///
/// 1. Reconstruct the COSE `Sig_structure` and verify the ES384 signature with
///    the leaf certificate's P-384 public key.
/// 2. Build and verify the X.509 path leaf -> `cabundle` -> Nitro Root G1,
///    checking validity windows, basic constraints, key usage, and that the
///    trust anchor's SHA-256 fingerprint equals
///    [`NITRO_ROOT_G1_SHA256_FINGERPRINT`].
///
/// Those steps require certificate-parsing and ES384 crypto dependencies and a
/// real document captured from a Nitro environment to test against, so they are
/// deliberately not implemented here. This function returns
/// [`PkiVerification::Stub`] to make the gap explicit and impossible to mistake
/// for a passing verification. Callers in production MUST refuse to trust a
/// `Stub` result.
pub fn verify_pki_chain(
    _doc: &NitroAttestationDoc,
    _envelope: &CoseSign1,
    _root_fingerprint: &[u8; 32],
) -> PkiVerification {
    // EXPERIMENTAL: no signature/chain verification performed. See doc comment.
    PkiVerification::Stub
}

// --- CBOR helpers (pure) ---

fn cbor_bytes(value: &ciborium::value::Value, field: &str) -> TeeResult<Vec<u8>> {
    value.as_bytes().cloned().ok_or_else(|| {
        TeeError::InvalidAttestationReport(format!("{field} is not a CBOR byte string"))
    })
}

fn optional_bytes(value: &ciborium::value::Value) -> Option<Vec<u8>> {
    if value.is_null() {
        return None;
    }
    value.as_bytes().cloned()
}

fn text_field(value: &ciborium::value::Value, field: &str) -> TeeResult<String> {
    value
        .as_text()
        .map(str::to_string)
        .ok_or_else(|| TeeError::InvalidAttestationReport(format!("{field} is not a CBOR string")))
}

fn uint_field(value: &ciborium::value::Value, field: &str) -> TeeResult<u64> {
    value
        .as_integer()
        .and_then(|i| u64::try_from(i).ok())
        .ok_or_else(|| {
            TeeError::InvalidAttestationReport(format!("{field} is not a non-negative integer"))
        })
}

fn parse_pcr_map(value: &ciborium::value::Value) -> TeeResult<BTreeMap<u8, Vec<u8>>> {
    let map = value
        .as_map()
        .ok_or_else(|| TeeError::InvalidAttestationReport("pcrs is not a CBOR map".to_string()))?;
    let mut out = BTreeMap::new();
    for (k, v) in map {
        let index = k
            .as_integer()
            .and_then(|i| u8::try_from(i).ok())
            .ok_or_else(|| {
                TeeError::InvalidAttestationReport("pcr index is not a u8".to_string())
            })?;
        let bytes = cbor_bytes(v, "pcr value")?;
        out.insert(index, bytes);
    }
    Ok(out)
}

fn parse_cert_array(value: &ciborium::value::Value) -> TeeResult<Vec<Vec<u8>>> {
    let array = value.as_array().ok_or_else(|| {
        TeeError::InvalidAttestationReport("cabundle is not a CBOR array".to_string())
    })?;
    let mut out = Vec::with_capacity(array.len());
    for entry in array {
        out.push(cbor_bytes(entry, "cabundle entry")?);
    }
    Ok(out)
}

fn missing(field: &str) -> TeeError {
    TeeError::InvalidAttestationReport(format!("missing required field: {field}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ciborium::value::Value;

    /// Builds a synthetic NSM payload CBOR map for tests.
    fn synthetic_payload(
        pcr0: Vec<u8>,
        pcr1: Vec<u8>,
        pcr2: Vec<u8>,
        with_optionals: bool,
    ) -> Vec<u8> {
        let pcr_map = Value::Map(vec![
            (Value::Integer(0.into()), Value::Bytes(pcr0)),
            (Value::Integer(1.into()), Value::Bytes(pcr1)),
            (Value::Integer(2.into()), Value::Bytes(pcr2)),
        ]);

        let mut entries = vec![
            (
                Value::Text("module_id".into()),
                Value::Text("i-0abc-enc01".into()),
            ),
            (Value::Text("digest".into()), Value::Text("SHA384".into())),
            (
                Value::Text("timestamp".into()),
                Value::Integer(1_700_000_000_000u64.into()),
            ),
            (Value::Text("pcrs".into()), pcr_map),
            (
                Value::Text("certificate".into()),
                Value::Bytes(vec![0x30, 0x82, 0x01, 0x02]), // fake DER prefix
            ),
            (
                Value::Text("cabundle".into()),
                Value::Array(vec![
                    Value::Bytes(vec![0x30, 0x82, 0x02, 0x01]),
                    Value::Bytes(vec![0x30, 0x82, 0x03, 0x01]),
                ]),
            ),
        ];

        if with_optionals {
            entries.push((
                Value::Text("public_key".into()),
                Value::Bytes(vec![0xAA; 8]),
            ));
            entries.push((
                Value::Text("user_data".into()),
                Value::Bytes(vec![0xBB; 16]),
            ));
            entries.push((Value::Text("nonce".into()), Value::Bytes(vec![0xCC; 12])));
        } else {
            entries.push((Value::Text("public_key".into()), Value::Null));
            entries.push((Value::Text("user_data".into()), Value::Null));
            entries.push((Value::Text("nonce".into()), Value::Null));
        }

        let map = Value::Map(entries);
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&map, &mut buf).unwrap();
        buf
    }

    /// Wraps a payload in a synthetic COSE_Sign1 4-element array.
    fn synthetic_cose(payload: Vec<u8>) -> Vec<u8> {
        let cose = Value::Array(vec![
            Value::Bytes(vec![0xA1, 0x01, 0x38, 0x22]), // protected: {1: -35} (ES384)
            Value::Map(vec![]),                         // unprotected
            Value::Bytes(payload),                      // payload
            Value::Bytes(vec![0u8; 96]),                // ES384 signature (P-384 r||s)
        ]);
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&cose, &mut buf).unwrap();
        buf
    }

    #[test]
    fn parses_cose_sign1_envelope() {
        let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
        let cose_bytes = synthetic_cose(payload.clone());

        let envelope = parse_cose_sign1(&cose_bytes).unwrap();
        assert_eq!(envelope.payload, payload);
        assert_eq!(envelope.signature.len(), 96);
        assert!(!envelope.protected.is_empty());
    }

    #[test]
    fn rejects_non_array_cose() {
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&Value::Text("nope".into()), &mut buf).unwrap();
        assert!(parse_cose_sign1(&buf).is_err());
    }

    #[test]
    fn rejects_wrong_arity_cose() {
        let three = Value::Array(vec![
            Value::Bytes(vec![]),
            Value::Map(vec![]),
            Value::Bytes(vec![]),
        ]);
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&three, &mut buf).unwrap();
        assert!(parse_cose_sign1(&buf).is_err());
    }

    #[test]
    fn parses_payload_with_optionals() {
        let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], true);
        let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();

        assert_eq!(doc.module_id, "i-0abc-enc01");
        assert_eq!(doc.digest, "SHA384");
        assert_eq!(doc.timestamp_ms, 1_700_000_000_000);
        assert_eq!(doc.pcrs.len(), 3);
        assert_eq!(doc.pcr(0).unwrap(), &[1u8; 48]);
        assert_eq!(doc.cabundle.len(), 2);
        assert_eq!(doc.public_key.as_deref(), Some([0xAA; 8].as_slice()));
        assert_eq!(doc.user_data.as_deref(), Some([0xBB; 16].as_slice()));
        assert_eq!(doc.nonce.as_deref(), Some([0xCC; 12].as_slice()));
    }

    #[test]
    fn parses_payload_with_null_optionals() {
        let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
        let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
        assert!(doc.public_key.is_none());
        assert!(doc.user_data.is_none());
        assert!(doc.nonce.is_none());
    }

    #[test]
    fn end_to_end_parse_from_cose() {
        let payload = synthetic_payload(vec![7u8; 48], vec![8u8; 48], vec![9u8; 48], true);
        let cose_bytes = synthetic_cose(payload);
        let doc = NitroAttestationDoc::parse(&cose_bytes).unwrap();
        assert_eq!(doc.module_id, "i-0abc-enc01");
        assert_eq!(doc.pcr(2).unwrap(), &[9u8; 48]);
    }

    #[test]
    fn structural_validate_accepts_well_formed_doc() {
        let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
        let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
        doc.structural_validate(&NitroValidationOptions::default())
            .unwrap();
    }

    #[test]
    fn structural_validate_rejects_zero_pcrs() {
        let payload = synthetic_payload(vec![0u8; 48], vec![0u8; 48], vec![0u8; 48], false);
        let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
        let err = doc
            .structural_validate(&NitroValidationOptions::default())
            .unwrap_err();
        assert!(matches!(err, TeeError::InvalidAttestationReport(_)));
    }

    #[test]
    fn structural_validate_rejects_wrong_pcr_length() {
        // Craft a payload whose PCR0 is the wrong length.
        let payload = synthetic_payload(vec![1u8; 47], vec![2u8; 48], vec![3u8; 48], false);
        let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
        assert!(
            doc.structural_validate(&NitroValidationOptions::default())
                .is_err()
        );
    }

    #[test]
    fn structural_validate_enforces_pcr_pin() {
        let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
        let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();

        // Matching pin passes.
        let mut opts = NitroValidationOptions::default();
        opts.expected_pcr0 = Some([1u8; 48]);
        doc.structural_validate(&opts).unwrap();

        // Mismatched pin fails.
        opts.expected_pcr0 = Some([9u8; 48]);
        assert!(doc.structural_validate(&opts).is_err());
    }

    #[test]
    fn structural_validate_rejects_wrong_digest() {
        // Build a payload manually with a non-SHA384 digest.
        let map = Value::Map(vec![
            (Value::Text("module_id".into()), Value::Text("m".into())),
            (Value::Text("digest".into()), Value::Text("SHA256".into())),
            (Value::Text("timestamp".into()), Value::Integer(1u64.into())),
            (
                Value::Text("pcrs".into()),
                Value::Map(vec![(
                    Value::Integer(0.into()),
                    Value::Bytes(vec![1u8; 48]),
                )]),
            ),
            (Value::Text("certificate".into()), Value::Bytes(vec![0x30])),
        ]);
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&map, &mut buf).unwrap();
        let doc = NitroAttestationDoc::parse_payload(&buf).unwrap();
        assert!(
            doc.structural_validate(&NitroValidationOptions::default())
                .is_err()
        );
    }

    #[test]
    fn parse_payload_rejects_missing_required_field() {
        // Missing certificate.
        let map = Value::Map(vec![
            (Value::Text("module_id".into()), Value::Text("m".into())),
            (Value::Text("digest".into()), Value::Text("SHA384".into())),
            (Value::Text("timestamp".into()), Value::Integer(1u64.into())),
            (
                Value::Text("pcrs".into()),
                Value::Map(vec![(
                    Value::Integer(0.into()),
                    Value::Bytes(vec![1u8; 48]),
                )]),
            ),
        ]);
        let mut buf = Vec::new();
        ciborium::ser::into_writer(&map, &mut buf).unwrap();
        assert!(NitroAttestationDoc::parse_payload(&buf).is_err());
    }

    #[test]
    fn verify_pki_chain_is_stub() {
        let payload = synthetic_payload(vec![1u8; 48], vec![2u8; 48], vec![3u8; 48], false);
        let cose_bytes = synthetic_cose(payload.clone());
        let envelope = parse_cose_sign1(&cose_bytes).unwrap();
        let doc = NitroAttestationDoc::parse_payload(&payload).unwrap();
        // The stub must NOT report Verified — guards against accidental "trust".
        assert_eq!(
            verify_pki_chain(&doc, &envelope, &NITRO_ROOT_G1_SHA256_FINGERPRINT),
            PkiVerification::Stub
        );
    }
}
