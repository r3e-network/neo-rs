//! PKCS#11-backed consensus signer.
//!
//! # Design
//!
//! The cryptoki [`Session`] type is `Send` but `!Sync` — it may only be called
//! from the thread that owns it.  The public [`Pkcs11Signer`] wrapper is both
//! `Send` and `Sync` because **all PKCS#11 calls are confined to one dedicated
//! worker thread** that exclusively owns the `Pkcs11` context and the `Session`.
//!
//! The worker thread communicates via a bounded `std::sync::mpsc` channel:
//!
//! ```text
//! ┌─────────────────────────────────┐         ┌───────────────────────────────────┐
//! │  Pkcs11Signer (Send+Sync)       │         │  Worker thread (owns Session)     │
//! │  ─ script_hash: UInt160         │         │  ─ Pkcs11 ctx                     │
//! │  ─ public_key:  [u8;33]         │  Cmd    │  ─ Session                        │
//! │  ─ tx: mpsc::Sender<Cmd>  ────────────────►  ─ ObjectHandle (priv key)        │
//! │                                 │  Reply  │                                   │
//! │  sign() → recv_timeout() ◄──────────────── session.sign(Mechanism::Ecdsa,..)  │
//! └─────────────────────────────────┘         └───────────────────────────────────┘
//! ```
//!
//! `sign()` is **synchronous** (no async, no async_trait) with a bounded
//! `SIGN_TIMEOUT`.  If the HSM stalls, the timeout fires and the consensus
//! driver receives an error → triggers change-view.
//!
//! # Signature post-processing
//!
//! Neo requires a 64-byte raw `r‖s` secp256r1 signature.  Post-processing:
//!
//! 1. If [`SigFormat::Der`] (GCP path): decode ASN.1 with
//!    `p256::ecdsa::Signature::from_der`.
//! 2. Parse raw bytes with `Signature::from_slice` (validates the curve).
//! 3. `Signature::normalize_s()` → canonical low-s (Neo C# parity, step is
//!    cheap and provider-independent).
//! 4. `sig.to_bytes()` → 64-byte `r‖s`.

use crate::config::{HsmConfig, SigFormat, profile};
use crate::error::{HsmError, HsmResult};
use neo_consensus::ConsensusSigner;
use neo_consensus::error::ConsensusError;
use neo_crypto::{Crypto, Secp256r1Crypto};
use neo_primitives::UInt160;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use tracing::{debug, info, warn};

use cryptoki::{
    context::{CInitializeArgs, CInitializeFlags, Pkcs11},
    mechanism::Mechanism,
    object::{Attribute, AttributeType, ObjectClass, ObjectHandle},
    session::{Session, UserType},
    types::AuthPin,
};

/// Maximum time `sign()` will block waiting for the HSM worker.
///
/// If the HSM does not respond within this window, `sign()` returns
/// [`HsmError::Timeout`], which the consensus driver treats as a transient
/// fault and triggers change-view.
const SIGN_TIMEOUT: Duration = Duration::from_secs(5);

/// DER OID bytes for the secp256r1 / prime256v1 curve (`1.2.840.10045.3.1.7`).
///
/// Used to validate that the key found on the token is on the correct curve.
const SECP256R1_DER_OID: &[u8] = &[0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];

/// Commands sent from [`Pkcs11Signer`] to the worker thread.
enum WorkerCmd {
    /// Sign the supplied SHA-256 digest.
    Sign {
        /// 32-byte SHA-256 digest of the data to sign.
        digest: [u8; 32],
        /// Private-key object handle (cached at init time, valid for session lifetime).
        key: ObjectHandle,
        /// One-shot reply channel.
        reply: mpsc::Sender<HsmResult<Vec<u8>>>,
    },
    /// Graceful shutdown.
    Shutdown,
}

/// A consensus signer backed by a PKCS#11 HSM.
///
/// Implements [`ConsensusSigner`] with a synchronous `sign()` that forwards
/// work to a dedicated worker thread that owns the PKCS#11 `Session`.
///
/// The struct itself is `Send + Sync` (the `Sender<WorkerCmd>` is both).
/// The `JoinHandle` is kept so that when the signer is dropped the worker
/// receives `Shutdown` and joins cleanly.
pub struct Pkcs11Signer {
    /// Channel to the session-owning worker thread.
    tx: mpsc::Sender<WorkerCmd>,
    /// Private-key handle (sent with every Sign command).
    key_handle: ObjectHandle,
    /// Compressed 33-byte secp256r1 public key (for external inspection).
    public_key: [u8; 33],
    /// Neo script hash (`UInt160::from_script(single_sig_redeem_script(pubkey))`).
    script_hash: UInt160,
    /// Worker thread join handle (dropped last so the worker can finish).
    _worker: thread::JoinHandle<()>,
    /// Signature format for post-processing (RawRs or Der).
    sig_format: SigFormat,
}

// Pkcs11Signer is automatically Send+Sync because:
// - mpsc::Sender<WorkerCmd> is Send+Sync
// - ObjectHandle is #[repr(transparent)] over CK_ULONG (usize): Send+Sync
// - [u8;33] and UInt160 are both Send+Sync
// - The Session lives exclusively in the worker thread and never escapes Pkcs11Signer
// No unsafe impls are needed or permitted (#![deny(unsafe_code)]).
const _: () = {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}
    fn check() {
        assert_send::<Pkcs11Signer>();
        assert_sync::<Pkcs11Signer>();
    }
    let _ = check;
};

impl Pkcs11Signer {
    /// Connect to the HSM and return a ready signer.
    ///
    /// This blocks briefly to:
    /// 1. `dlopen` the PKCS#11 library.
    /// 2. `C_Initialize`.
    /// 3. Pick the slot (by `slot` number, `token_label`, or first available).
    /// 4. `C_OpenSession` (read-only is sufficient for signing).
    /// 5. `C_Login(CKU_USER, pin)`.
    /// 6. Locate the private key by `CKA_LABEL` (and `CKA_ID` if set).
    /// 7. Read the matching public key's `CKA_EC_POINT`, decode the compressed
    ///    33-byte secp256r1 pubkey, compute the Neo `UInt160` script hash.
    ///
    /// Fails fast on any error before consensus starts.
    pub fn connect(cfg: &HsmConfig) -> HsmResult<Self> {
        let prof = profile(cfg.provider);
        let sig_format = prof.signature_format;
        let library_path = cfg.library_path.clone();
        let slot_cfg = cfg.slot;
        let token_label_cfg = cfg.token_label.clone();
        let key_label = cfg.key_label.clone();
        let key_id = cfg.key_id.clone();
        let pin_str = cfg.user_pin.clone();

        // One-shot channel: worker sends back (key_handle, pubkey, script_hash)
        // once init succeeds, or the error.
        let (init_tx, init_rx) = mpsc::channel::<HsmResult<(ObjectHandle, [u8; 33], UInt160)>>();

        // Signing command channel.
        let (cmd_tx, cmd_rx) = mpsc::channel::<WorkerCmd>();

        let worker = thread::Builder::new()
            .name("neo-hsm-pkcs11-worker".to_string())
            .spawn(move || {
                // ── Init ──────────────────────────────────────────────────────
                let result = Self::worker_init(
                    &library_path,
                    slot_cfg,
                    token_label_cfg.as_deref(),
                    &key_label,
                    key_id.as_deref(),
                    &pin_str,
                );
                let (session, _key_handle, pubkey, _script_hash) = match result {
                    Ok(v) => {
                        let _ = init_tx.send(Ok((v.1, v.2, v.3)));
                        v
                    }
                    Err(e) => {
                        let _ = init_tx.send(Err(e));
                        return;
                    }
                };

                // ── Sign loop ─────────────────────────────────────────────────
                info!(
                    pubkey = hex::encode(pubkey),
                    "neo-hsm: PKCS#11 worker ready"
                );
                for cmd in cmd_rx {
                    match cmd {
                        WorkerCmd::Sign { digest, key, reply } => {
                            let res = session
                                .sign(&Mechanism::Ecdsa, key, &digest)
                                .map_err(HsmError::Pkcs11);
                            let _ = reply.send(res);
                        }
                        WorkerCmd::Shutdown => break,
                    }
                }
                debug!("neo-hsm: PKCS#11 worker shutting down");
            })
            .map_err(|e| HsmError::Init(format!("failed to spawn HSM worker thread: {e}")))?;

        // Block here until the worker completes init.
        let (key_handle, pubkey, script_hash) = init_rx
            .recv()
            .map_err(|_| HsmError::Init("worker thread died before init completed".into()))??;

        Ok(Self {
            tx: cmd_tx,
            key_handle,
            public_key: pubkey,
            script_hash,
            _worker: worker,
            sig_format,
        })
    }

    /// Returns the compressed 33-byte secp256r1 public key.
    #[must_use]
    pub fn public_key(&self) -> &[u8; 33] {
        &self.public_key
    }

    /// Returns the Neo script hash derived from the public key.
    #[must_use]
    pub fn script_hash(&self) -> &UInt160 {
        &self.script_hash
    }

    /// Worker-side initialization: load library, open session, login, find key.
    ///
    /// Returns `(session, key_handle, compressed_pubkey_33, script_hash)`.
    #[allow(clippy::too_many_lines)] // Initialization is inherently sequential.
    fn worker_init(
        library_path: &std::path::Path,
        slot_cfg: Option<u64>,
        token_label: Option<&str>,
        key_label: &str,
        key_id: Option<&[u8]>,
        pin_str: &str,
    ) -> HsmResult<(Session, ObjectHandle, [u8; 33], UInt160)> {
        // 1. Load the PKCS#11 library.
        let pkcs11 = Pkcs11::new(library_path)
            .map_err(|e| HsmError::Init(format!("dlopen {}: {e}", library_path.display())))?;

        // 2. C_Initialize with OS threading.
        pkcs11
            .initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK))
            .map_err(|e| HsmError::Init(format!("C_Initialize: {e}")))?;

        // 3. Pick slot.
        let slot = if let Some(idx) = slot_cfg {
            // Use the explicit slot index as-is.
            pkcs11
                .get_slots_with_token()
                .map_err(|e| HsmError::Init(format!("C_GetSlotList: {e}")))?
                .into_iter()
                .nth(idx as usize)
                .ok_or_else(|| HsmError::Init(format!("slot index {idx} out of range")))?
        } else if let Some(label) = token_label {
            // Find the slot whose token label matches.
            let slots = pkcs11
                .get_slots_with_token()
                .map_err(|e| HsmError::Init(format!("C_GetSlotList: {e}")))?;
            let mut found = None;
            for s in slots {
                if let Ok(info) = pkcs11.get_token_info(s) {
                    let token_label_trimmed = info.label().trim();
                    if token_label_trimmed == label.trim() {
                        found = Some(s);
                        break;
                    }
                }
            }
            found.ok_or_else(|| HsmError::Init(format!("no token with label '{label}' found")))?
        } else {
            // First available slot.
            pkcs11
                .get_slots_with_token()
                .map_err(|e| HsmError::Init(format!("C_GetSlotList: {e}")))?
                .into_iter()
                .next()
                .ok_or_else(|| HsmError::Init("no HSM slots with token present".into()))?
        };

        // 4. Open a read-only session (signing does not require RW).
        let session = pkcs11
            .open_ro_session(slot)
            .map_err(|e| HsmError::Init(format!("C_OpenSession: {e}")))?;

        // 5. Login as CKU_USER.
        let auth_pin = AuthPin::from(pin_str);
        session
            .login(UserType::User, Some(&auth_pin))
            .map_err(|e| HsmError::Login(format!("C_Login: {e}")))?;
        drop(auth_pin); // zeroize immediately

        // 6. Find the private key by CKA_LABEL (+ optional CKA_ID).
        let mut priv_template = vec![
            Attribute::Label(key_label.as_bytes().to_vec()),
            Attribute::Class(ObjectClass::PRIVATE_KEY),
        ];
        if let Some(id) = key_id {
            priv_template.push(Attribute::Id(id.to_vec()));
        }
        let priv_objects = session
            .find_objects(&priv_template)
            .map_err(|e| HsmError::Sign(format!("C_FindObjects(private): {e}")))?;
        let priv_handle = priv_objects
            .into_iter()
            .next()
            .ok_or_else(|| HsmError::KeyNotFound {
                label: key_label.to_string(),
            })?;

        // 7. Find the matching public key and read its EC point.
        let mut pub_template = vec![
            Attribute::Label(key_label.as_bytes().to_vec()),
            Attribute::Class(ObjectClass::PUBLIC_KEY),
        ];
        if let Some(id) = key_id {
            pub_template.push(Attribute::Id(id.to_vec()));
        }
        let pub_objects = session
            .find_objects(&pub_template)
            .map_err(|e| HsmError::PublicKey(format!("C_FindObjects(public): {e}")))?;
        let pub_handle = pub_objects
            .into_iter()
            .next()
            .ok_or_else(|| HsmError::PublicKey(format!("no public key for label '{key_label}'")))?;

        // Read CKA_EC_PARAMS and CKA_EC_POINT.
        let attrs = session
            .get_attributes(
                pub_handle,
                &[AttributeType::EcParams, AttributeType::EcPoint],
            )
            .map_err(|e| HsmError::PublicKey(format!("C_GetAttributeValue: {e}")))?;

        let mut ec_params_bytes: Option<Vec<u8>> = None;
        let mut ec_point_bytes: Option<Vec<u8>> = None;
        for attr in attrs {
            match attr {
                Attribute::EcParams(b) => ec_params_bytes = Some(b),
                Attribute::EcPoint(b) => ec_point_bytes = Some(b),
                _ => {}
            }
        }

        // Validate curve: ec_params must contain the secp256r1 OID.
        if let Some(ref params) = ec_params_bytes {
            if !params
                .windows(SECP256R1_DER_OID.len())
                .any(|w| w == SECP256R1_DER_OID)
            {
                return Err(HsmError::PublicKey(format!(
                    "key '{key_label}' is not on secp256r1: ec_params={}",
                    hex::encode(params)
                )));
            }
        } else {
            warn!("neo-hsm: CKA_EC_PARAMS not returned; skipping curve check");
        }

        // Decode CKA_EC_POINT: PKCS#11 wraps the point in a DER OCTET STRING.
        // The X9.62 uncompressed form is `04 || x(32) || y(32)` = 65 bytes;
        // compressed form is `02/03 || x(32)` = 33 bytes.
        let ec_point_raw = ec_point_bytes
            .ok_or_else(|| HsmError::PublicKey("CKA_EC_POINT attribute not returned".into()))?;
        let point_bytes = decode_der_octet_string(&ec_point_raw)?;
        let compressed_pubkey = normalize_to_compressed(&point_bytes)?;

        // 8. Compute Neo UInt160 script hash from the compressed public key.
        // Neo single-sig redeem script: `PUSHDATA1(33) || pubkey || SYSCALL CheckSig`
        let script = signature_redeem_script(&compressed_pubkey);
        let script_hash = UInt160::from_script(&script);

        info!(
            label = key_label,
            pubkey = hex::encode(compressed_pubkey),
            script_hash = %script_hash,
            "neo-hsm: key located on HSM"
        );

        Ok((session, priv_handle, compressed_pubkey, script_hash))
    }

    /// Internal sign path: compute digest, send to worker, await result,
    /// post-process to canonical 64-byte low-s `r‖s`.
    fn do_sign(&self, data: &[u8]) -> HsmResult<Vec<u8>> {
        // Neo's consensus signs SHA-256(data); CKM_ECDSA expects the raw digest.
        let digest = Crypto::sha256(data);

        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .send(WorkerCmd::Sign {
                digest,
                key: self.key_handle,
                reply: reply_tx,
            })
            .map_err(|_| HsmError::Disconnected)?;

        // Bounded wait — a stalled HSM must not wedge the dBFT driver.
        let raw = reply_rx
            .recv_timeout(SIGN_TIMEOUT)
            .map_err(|_| HsmError::Timeout(SIGN_TIMEOUT))??;

        // Post-process: DER → r||s (GCP), then low-s normalize (all providers).
        finalize_signature(&raw, self.sig_format)
    }
}

impl Drop for Pkcs11Signer {
    fn drop(&mut self) {
        // Best-effort shutdown signal; ignore if the worker is already gone.
        let _ = self.tx.send(WorkerCmd::Shutdown);
    }
}

impl ConsensusSigner for Pkcs11Signer {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        *script_hash == self.script_hash
    }

    fn sign(&self, data: &[u8], script_hash: &UInt160) -> Result<Vec<u8>, ConsensusError> {
        if !self.can_sign(script_hash) {
            return Err(ConsensusError::state_error(format!(
                "hsm-pkcs11: unknown script hash {script_hash}"
            )));
        }
        self.do_sign(data).map_err(ConsensusError::from)
    }
}

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Finalize raw HSM output into 64-byte canonical low-s `r‖s`.
///
/// * `SigFormat::Der` (GCP): decode DER → `r‖s` via `p256::ecdsa::Signature::from_der`.
/// * `SigFormat::RawRs` (AWS/Azure): parse with `Signature::from_slice`.
/// * Both paths: `normalize_s()` → low half-order `s`.
fn finalize_signature(raw: &[u8], format: SigFormat) -> HsmResult<Vec<u8>> {
    // Shared canonicalization (DER-decode if needed + low-s) lives in neo-crypto
    // so every external signer backend (PKCS#11, Ledger, …) reuses one path.
    Secp256r1Crypto::canonicalize_signature(raw, matches!(format, SigFormat::Der))
        .map(|sig| sig.to_vec())
        .map_err(|e| HsmError::SigDecode(format!("{e}")))
}

/// Decode a PKCS#11 `CKA_EC_POINT` attribute value.
///
/// The PKCS#11 spec wraps the X9.62 public-key octet string in a DER
/// `OCTET STRING` header (`0x04 <len> <point>`).  Strip the header and
/// return the raw point bytes.
fn decode_der_octet_string(raw: &[u8]) -> HsmResult<Vec<u8>> {
    // A bare X9.62 point is exactly 33 bytes (compressed) or 65 bytes
    // (uncompressed); a DER OCTET STRING wrapping one is 35 or 67 bytes
    // (a 2-byte short-form header `04 <len>`). Those four sizes never collide,
    // so length alone disambiguates wrapped-vs-bare. We MUST NOT key off the
    // tag byte: an uncompressed point itself begins with 0x04, which equals the
    // OCTET STRING tag, so a bare point would otherwise be misread as wrapped.
    if raw.len() == 33 || raw.len() == 65 {
        return Ok(raw.to_vec());
    }
    // Otherwise expect a DER OCTET STRING: tag=0x04, length byte(s), content.
    if raw.len() < 2 || raw[0] != 0x04 {
        // Some tokens already strip the DER wrapper and return the raw point.
        return Ok(raw.to_vec());
    }
    let (content_offset, content_len) = if raw[1] & 0x80 == 0 {
        // Short form: length in 1 byte.
        (2usize, raw[1] as usize)
    } else {
        // Long form: next (raw[1] & 0x7f) bytes are the length.
        let num_len_bytes = (raw[1] & 0x7f) as usize;
        if raw.len() < 2 + num_len_bytes {
            return Err(HsmError::PublicKey("CKA_EC_POINT DER truncated".into()));
        }
        let mut len = 0usize;
        for &b in &raw[2..2 + num_len_bytes] {
            len = (len << 8) | b as usize;
        }
        (2 + num_len_bytes, len)
    };
    // Only strip the header when the declared length matches the buffer exactly;
    // a mismatch means this isn't really a DER wrapper (e.g. a bare point whose
    // leading coordinate byte happens to look like a length), so pass it through.
    if content_offset + content_len != raw.len() {
        return Ok(raw.to_vec());
    }
    Ok(raw[content_offset..].to_vec())
}

/// Accept an X9.62 public-key point (either uncompressed 65-byte `04‖x‖y` or
/// already-compressed 33-byte `02/03‖x`) and return the 33-byte compressed form.
fn normalize_to_compressed(point: &[u8]) -> HsmResult<[u8; 33]> {
    match point.len() {
        33 => {
            // Already compressed.
            let mut out = [0u8; 33];
            out.copy_from_slice(point);
            Ok(out)
        }
        65 if point[0] == 0x04 => {
            // Uncompressed: derive parity from y coordinate.
            let prefix: u8 = if point[64] & 1 == 0 { 0x02 } else { 0x03 };
            let mut out = [0u8; 33];
            out[0] = prefix;
            out[1..].copy_from_slice(&point[1..33]);
            Ok(out)
        }
        other => Err(HsmError::PublicKey(format!(
            "unexpected EC point length {other} (expected 33 or 65)"
        ))),
    }
}

/// Build the Neo single-sig verification/redeem script for a compressed pubkey.
///
/// Encoding mirrors C# `Contract.CreateSignatureRedeemScript`:
/// `0x21 <33-byte pubkey> 0x41 <4-byte System.Crypto.CheckSig hash>`
///
/// The `UInt160::from_script()` of this byte sequence is the validator's
/// consensus script hash (the value returned by `can_sign` / stored in
/// `NextConsensus`).
fn signature_redeem_script(compressed_pubkey: &[u8; 33]) -> Vec<u8> {
    // NeoVM opcodes:
    //   0x0C = PUSHDATA1 with 1-byte length prefix (NEOV3)
    //   0x21 = length of the pubkey (33)
    //   ...33 bytes...
    //   0x41 = SYSCALL opcode
    //   System.Crypto.CheckSig interop hash (4 bytes, little-endian)
    //
    // This matches C# `Contract.CreateSignatureRedeemScript` which emits:
    //   ScriptBuilder.EmitPush(publicKey) + ScriptBuilder.EmitSysCall("System.Crypto.CheckSig")
    //
    // neo-consensus uses multisig_verification_script which wraps m-of-n;
    // here we build the single-sig variant for HSM key identity.
    let mut script = Vec::with_capacity(40);
    // PUSHDATA1  (NeVM3 opcode 0x0C)
    script.push(0x0C);
    // Length byte for 33-byte pubkey
    script.push(0x21);
    // The 33-byte compressed public key
    script.extend_from_slice(compressed_pubkey);
    // SYSCALL opcode (0x41)
    script.push(0x41);
    // System.Crypto.CheckSig interop descriptor hash: the first 4 bytes of
    // sha256("System.Crypto.CheckSig"), little-endian. C# registers the syscall
    // under BitConverter.ToUInt32(sha256(name)[..4]) = 0x27b3e756, i.e. bytes
    // [0x56, 0xe7, 0xb3, 0x27]. Derived here so it can never drift from the hash.
    let checksig_hash = Crypto::sha256(b"System.Crypto.CheckSig");
    script.extend_from_slice(&checksig_hash[..4]);
    script
}

#[cfg(test)]
#[path = "../tests/providers/pkcs11.rs"]
mod tests;
