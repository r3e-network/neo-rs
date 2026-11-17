//! Authenticated client identity of an mTLS peer.
//!
//! The signer only ever speaks HTTPS with mutual TLS, so a request that
//! reaches the HTTP layer was already authenticated by the configured CA.
//! Authentication is not authorization though: the certificate tells us *who*
//! the caller is (its subject common name), and [`crate::config::ClientAuthorizationPolicy`]
//! decides which key IDs and purposes that identity may use.
//!
//! The certificate common name is extracted with a minimal DER walker instead
//! of a full X.509 stack, because the signer only needs one attribute. The
//! walker is strict: it never searches for byte patterns, it navigates
//! `Certificate ::= SEQUENCE { tbsCertificate, ... }` and
//! `tbsCertificate.subject` (`Name ::= SEQUENCE OF RelativeDistinguishedName`)
//! by structure.

use neo_crypto::Crypto;
use std::fmt;

/// OID 2.5.4.3 (id-at-commonName) encoded as a DER OBJECT IDENTIFIER body.
const OID_COMMON_NAME: [u8; 3] = [0x55, 0x04, 0x03];

/// ASN.1/DER tags used while walking a certificate.
const TAG_SEQUENCE: u8 = 0x30;
const TAG_SET: u8 = 0x31;
const TAG_OID: u8 = 0x06;
const TAG_CONTEXT_0: u8 = 0xA0;
const TAG_UTF8_STRING: u8 = 0x0C;
const TAG_PRINTABLE_STRING: u8 = 0x13;
const TAG_IA5_STRING: u8 = 0x16;
const TAG_BMP_STRING: u8 = 0x1E;

/// An authenticated mutual-TLS client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientIdentity {
    common_name: String,
    certificate_sha256: [u8; 32],
}

impl ClientIdentity {
    /// Builds an identity from an explicit common name and certificate
    /// fingerprint.
    #[must_use]
    pub fn new(common_name: impl Into<String>, certificate_sha256: [u8; 32]) -> Self {
        Self {
            common_name: common_name.into(),
            certificate_sha256,
        }
    }

    /// Derives an identity from the DER encoding of the client's leaf
    /// certificate.
    ///
    /// Returns `None` when the certificate carries no subject common name: an
    /// unnamed peer cannot be scoped to a grant, so it must never be
    /// authorized.
    #[must_use]
    pub fn from_certificate_der(der: &[u8]) -> Option<Self> {
        let common_name = common_name_from_certificate(der)?;
        Some(Self {
            common_name,
            certificate_sha256: Crypto::sha256(der),
        })
    }

    /// Builds an identity from a common name alone (used by in-process tests
    /// and callers that already authenticated the peer).
    #[must_use]
    pub fn from_common_name(common_name: impl Into<String>) -> Self {
        let common_name = common_name.into();
        let certificate_sha256 = Crypto::sha256(common_name.as_bytes());
        Self {
            common_name,
            certificate_sha256,
        }
    }

    /// The certificate subject common name.
    #[must_use]
    pub fn common_name(&self) -> &str {
        &self.common_name
    }

    /// SHA-256 fingerprint of the presented certificate, for audit logs.
    #[must_use]
    pub fn certificate_sha256(&self) -> &[u8; 32] {
        &self.certificate_sha256
    }
}

impl fmt::Display for ClientIdentity {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.common_name)
    }
}

/// Reads one DER tag/length/value triple, returning the tag, the value and the
/// unconsumed remainder.
fn read_tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let tag = *input.first()?;
    let (length, length_bytes) = read_length(input.get(1..)?)?;
    let value_start = 1usize.checked_add(length_bytes)?;
    let value_end = value_start.checked_add(length)?;
    if input.len() < value_end {
        return None;
    }
    Some((tag, &input[value_start..value_end], &input[value_end..]))
}

/// Reads a DER definite length, returning the value and the length-field size.
fn read_length(input: &[u8]) -> Option<(usize, usize)> {
    let first = *input.first()?;
    if first < 0x80 {
        return Some((usize::from(first), 1));
    }
    let octets = usize::from(first & 0x7F);
    if octets == 0 || octets > 4 || input.len() < octets + 1 {
        // Indefinite/oversized lengths are not valid DER for a certificate.
        return None;
    }
    let mut length = 0usize;
    for byte in &input[1..=octets] {
        length = length.checked_mul(256)?.checked_add(usize::from(*byte))?;
    }
    Some((length, octets + 1))
}

/// Extracts `subject.commonName` from a DER-encoded X.509 certificate.
#[must_use]
pub fn common_name_from_certificate(der: &[u8]) -> Option<String> {
    let (tag, certificate, _) = read_tlv(der)?;
    if tag != TAG_SEQUENCE {
        return None;
    }
    let (tag, tbs_certificate, _) = read_tlv(certificate)?;
    if tag != TAG_SEQUENCE {
        return None;
    }

    // TBSCertificate ::= SEQUENCE {
    //   version [0] EXPLICIT Version DEFAULT v1, serialNumber, signature,
    //   issuer, validity, subject, subjectPublicKeyInfo, ... }
    let mut fields: Vec<(u8, &[u8])> = Vec::new();
    let mut rest = tbs_certificate;
    while let Some((tag, value, next)) = read_tlv(rest) {
        fields.push((tag, value));
        rest = next;
    }
    let subject_index = if fields.first().map(|field| field.0) == Some(TAG_CONTEXT_0) {
        5
    } else {
        4
    };
    let (subject_tag, subject) = *fields.get(subject_index)?;
    if subject_tag != TAG_SEQUENCE {
        return None;
    }

    // Name ::= SEQUENCE OF RelativeDistinguishedName
    // RelativeDistinguishedName ::= SET OF AttributeTypeAndValue
    // AttributeTypeAndValue ::= SEQUENCE { type OBJECT IDENTIFIER, value ANY }
    let mut rdns = subject;
    while let Some((tag, rdn, next)) = read_tlv(rdns) {
        rdns = next;
        if tag != TAG_SET {
            continue;
        }
        let mut attributes = rdn;
        while let Some((tag, attribute, next)) = read_tlv(attributes) {
            attributes = next;
            if tag != TAG_SEQUENCE {
                continue;
            }
            let (oid_tag, oid, remainder) = read_tlv(attribute)?;
            if oid_tag != TAG_OID || oid != OID_COMMON_NAME {
                continue;
            }
            let (value_tag, value, _) = read_tlv(remainder)?;
            return decode_directory_string(value_tag, value);
        }
    }
    None
}

/// Decodes the string types RFC 5280 allows for a directory string.
fn decode_directory_string(tag: u8, value: &[u8]) -> Option<String> {
    match tag {
        TAG_UTF8_STRING | TAG_PRINTABLE_STRING | TAG_IA5_STRING => {
            let text = std::str::from_utf8(value).ok()?.trim();
            if text.is_empty() {
                None
            } else {
                Some(text.to_string())
            }
        }
        TAG_BMP_STRING if value.len() % 2 == 0 => {
            let units: Vec<u16> = value
                .chunks_exact(2)
                .map(|pair| u16::from_be_bytes([pair[0], pair[1]]))
                .collect();
            let text = String::from_utf16(&units).ok()?.trim().to_string();
            if text.is_empty() { None } else { Some(text) }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tlv(tag: u8, value: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        assert!(value.len() < 0x80, "test DER uses short-form lengths only");
        out.push(value.len() as u8);
        out.extend_from_slice(value);
        out
    }

    fn attribute(oid: &[u8], tag: u8, value: &[u8]) -> Vec<u8> {
        let mut body = tlv(TAG_OID, oid);
        body.extend_from_slice(&tlv(tag, value));
        tlv(TAG_SEQUENCE, &body)
    }

    fn certificate(subject_attributes: &[Vec<u8>]) -> Vec<u8> {
        let issuer = tlv(TAG_SEQUENCE, &[attribute(&[0x55, 0x04, 0x0A], TAG_UTF8_STRING, b"issuer")].concat());
        let validity = tlv(TAG_SEQUENCE, &[]);
        let subject = tlv(TAG_SEQUENCE, &subject_attributes.concat());
        let mut tbs = tlv(TAG_CONTEXT_0, &tlv(0x02, &[0x02]));
        tbs.extend_from_slice(&tlv(0x02, &[0x01]));
        tbs.extend_from_slice(&tlv(TAG_SEQUENCE, &[]));
        tbs.extend_from_slice(&issuer);
        tbs.extend_from_slice(&validity);
        tbs.extend_from_slice(&subject);
        tlv(TAG_SEQUENCE, &tlv(TAG_SEQUENCE, &tbs))
    }

    #[test]
    fn extracts_the_common_name_of_a_v3_certificate() {
        let cert = certificate(&[tlv(
            TAG_SET,
            &[
                attribute(&[0x55, 0x04, 0x03], TAG_UTF8_STRING, b"neo-node-1"),
                attribute(&[0x55, 0x04, 0x0A], TAG_UTF8_STRING, b"Neo"),
            ]
            .concat(),
        )]);
        assert_eq!(
            common_name_from_certificate(&cert).as_deref(),
            Some("neo-node-1")
        );
    }

    #[test]
    fn extracts_the_common_name_of_a_v1_certificate() {
        let issuer = tlv(
            TAG_SEQUENCE,
            &[attribute(&[0x55, 0x04, 0x03], TAG_PRINTABLE_STRING, b"root")].concat(),
        );
        let subject = tlv(
            TAG_SEQUENCE,
            &[attribute(&[0x55, 0x04, 0x03], TAG_PRINTABLE_STRING, b"legacy-client")].concat(),
        );
        let mut tbs = tlv(0x02, &[0x01]);
        tbs.extend_from_slice(&tlv(TAG_SEQUENCE, &[]));
        tbs.extend_from_slice(&issuer);
        tbs.extend_from_slice(&tlv(TAG_SEQUENCE, &[]));
        tbs.extend_from_slice(&subject);
        let cert = tlv(TAG_SEQUENCE, &tlv(TAG_SEQUENCE, &tbs));
        assert_eq!(
            common_name_from_certificate(&cert).as_deref(),
            Some("legacy-client")
        );
    }

    #[test]
    fn certificates_without_a_common_name_yield_no_identity() {
        let cert = certificate(&[tlv(
            TAG_SET,
            &[attribute(&[0x55, 0x04, 0x0A], TAG_UTF8_STRING, b"Neo")].concat(),
        )]);
        assert!(common_name_from_certificate(&cert).is_none());
        assert!(ClientIdentity::from_certificate_der(&cert).is_none());
    }

    #[test]
    fn truncated_certificates_are_rejected() {
        let cert = certificate(&[tlv(
            TAG_SET,
            &[attribute(&[0x55, 0x04, 0x03], TAG_UTF8_STRING, b"neo-node-1")].concat(),
        )]);
        for length in [0, 1, cert.len() / 2, cert.len() - 1] {
            assert!(common_name_from_certificate(&cert[..length]).is_none());
        }
    }

    #[test]
    fn identity_fingerprints_the_presented_certificate() {
        let cert = certificate(&[tlv(
            TAG_SET,
            &[attribute(&[0x55, 0x04, 0x03], TAG_UTF8_STRING, b"neo-node-1")].concat(),
        )]);
        let identity = ClientIdentity::from_certificate_der(&cert).expect("identity");
        assert_eq!(identity.common_name(), "neo-node-1");
        assert_eq!(*identity.certificate_sha256(), Crypto::sha256(&cert));
    }
}
