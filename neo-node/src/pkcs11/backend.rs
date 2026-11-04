//! Backend abstraction over a PKCS#11 token used by the hardware TEE signer.

/// ECDSA mechanism negotiated with the token for Neo P-256 signatures.
///
/// Neo N3 consensus and state-root signatures are ECDSA P-256 over the
/// SHA-256 digest of canonical input, so only digest-capable ECDSA
/// mechanisms are relevant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Pkcs11Mechanism {
    /// `CKM_ECDSA` (0x1041) over a caller-supplied 32-byte SHA-256 digest.
    Ecdsa,
    /// `CKM_ECDSA_SHA256` (0x1044) over the canonical payload bytes.
    EcdsaSha256,
}

impl Pkcs11Mechanism {
    /// Raw PKCS#11 mechanism constant.
    pub(crate) const fn raw_code(self) -> u64 {
        match self {
            Self::Ecdsa => 0x0000_1041,
            Self::EcdsaSha256 => 0x0000_1044,
        }
    }
}

/// Slot, token, and key addressing for one HSM key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Pkcs11KeyQuery {
    /// Slot identifier that must hold a token with the declared label.
    pub(crate) slot_id: u64,
    /// Token label that must match the discovered token.
    pub(crate) token_label: String,
    /// `CKA_LABEL` of the EC P-256 private/public key pair.
    pub(crate) key_label: String,
}

/// One fully addressed user login and signing call into a PKCS#11 token.
#[derive(Debug, Clone)]
pub(crate) struct Pkcs11SignCall {
    /// Slot identifier of the token holding the private key.
    pub(crate) slot_id: u64,
    /// Token label required to match before signing.
    pub(crate) token_label: String,
    /// `CKA_LABEL` of the private key object.
    pub(crate) key_label: String,
    /// HSM user PIN (`CKU_USER` login credential).
    pub(crate) pin: Vec<u8>,
    /// Negotiated ECDSA mechanism.
    pub(crate) mechanism: Pkcs11Mechanism,
    /// Canonical bytes (`CKM_ECDSA_SHA256`) or 32-byte digest (`CKM_ECDSA`).
    pub(crate) data: Vec<u8>,
}

/// Minimal PKCS#11 operations required by the hardware TEE signer.
///
/// Implementations must be safe to share across async tasks; the FFI backend
/// relies on `CKF_OS_LOCKING_OK` initialization for thread safety.
pub(crate) trait Pkcs11Backend: Send + Sync {
    /// Reports whether the token in `slot_id` implements the mechanism.
    fn supports_mechanism(&self, slot_id: u64, mechanism: Pkcs11Mechanism) -> bool;

    /// Discovers the token and exports the compressed P-256 public key bound
    /// to the configured key label.
    fn export_public_key(&self, query: &Pkcs11KeyQuery) -> anyhow::Result<Vec<u8>>;

    /// Logs in the user and signs, returning the raw 64-byte `r || s`
    /// signature.
    fn sign(&self, call: &Pkcs11SignCall) -> anyhow::Result<Vec<u8>>;
}
