//! SGX hardware attestation and evidence helpers.
//!
//! This module provides strict, fail-closed SGX evidence loading for `sgx-hw` mode.

use crate::attestation::Quote;
use crate::error::{EnclaveInitError, TeeError, TeeResult};
use libloading::{Library, Symbol};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{fs, os::raw::c_long};
use tracing::{debug, info, warn};

const SGX_QL_SUCCESS: u32 = 0x0000;
const SGX_QL_QV_RESULT_OK: u32 = 0x0000;
const SGX_QL_QV_RESULT_CONFIG_NEEDED: u32 = 0x0000_A001;
const SGX_QL_QV_RESULT_OUT_OF_DATE: u32 = 0x0000_A002;
const SGX_QL_QV_RESULT_OUT_OF_DATE_CONFIG_NEEDED: u32 = 0x0000_A003;
const SGX_QL_QV_RESULT_INVALID_SIGNATURE: u32 = 0x0000_A004;
const SGX_QL_QV_RESULT_REVOKED: u32 = 0x0000_A005;
const SGX_QL_QV_RESULT_UNSPECIFIED: u32 = 0x0000_A006;
const SGX_QL_QV_RESULT_SW_HARDENING_NEEDED: u32 = 0x0000_A007;
const SGX_QL_QV_RESULT_CONFIG_AND_SW_HARDENING_NEEDED: u32 = 0x0000_A008;

const SGX_QUOTEVERIFY_LIB_NAMES: [&str; 2] =
    ["libsgx_dcap_quoteverify.so.1", "libsgx_dcap_quoteverify.so"];

pub(crate) const ENV_SGX_QUOTE_PATH: &str = "NEO_TEE_SGX_QUOTE_PATH";
pub(crate) const ENV_SGX_QUOTE_HEX: &str = "NEO_TEE_SGX_QUOTE_HEX";
pub(crate) const ENV_SGX_SEALING_KEY_PATH: &str = "NEO_TEE_SGX_SEALING_KEY_PATH";
pub(crate) const ENV_SGX_SEALING_KEY_HEX: &str = "NEO_TEE_SGX_SEALING_KEY_HEX";
pub(crate) const ENV_SGX_ALLOW_NON_TERMINAL_QV: &str = "NEO_TEE_SGX_ALLOW_NON_TERMINAL_QV";
pub(crate) const ENV_SGX_ALLOW_EXPIRED_COLLATERAL: &str = "NEO_TEE_SGX_ALLOW_EXPIRED_COLLATERAL";
pub(crate) const ENV_SGX_QV_LIB_PATH: &str = "NEO_TEE_SGX_QV_LIB_PATH";

#[derive(Debug, Clone)]
pub(crate) struct VerifiedSgxEvidence {
    pub quote: Vec<u8>,
    pub mrenclave: [u8; 32],
    pub mrsigner: [u8; 32],
    pub isv_prod_id: u16,
    pub isv_svn: u16,
    pub report_data: [u8; 64],
    pub cpu_svn: [u8; 16],
    pub attributes: [u8; 16],
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct DcapVerificationOutcome {
    pub collateral_expiration_status: u32,
    pub quote_verification_result: u32,
}

#[derive(Debug, Clone)]
pub(crate) struct VerifiedSgxRuntimeMaterial {
    pub evidence: VerifiedSgxEvidence,
    pub sealing_key: [u8; 32],
}

type SgxQvVerifyQuote = unsafe extern "C" fn(
    p_quote: *const u8,
    quote_size: u32,
    p_quote_collateral: *const std::ffi::c_void,
    expiration_check_date: c_long,
    p_collateral_expiration_status: *mut u32,
    p_quote_verification_result: *mut u32,
    p_qve_report_info: *mut std::ffi::c_void,
    supplemental_data_size: u32,
    p_supplemental_data: *mut u8,
) -> u32;

pub(crate) fn verify_runtime_evidence(
    sealed_data_path: &Path,
    expected_mrenclave: Option<[u8; 32]>,
    expected_mrsigner: Option<[u8; 32]>,
    min_isv_svn: u16,
    allow_debug_in_production: bool,
) -> TeeResult<VerifiedSgxRuntimeMaterial> {
    ensure_hardware_prerequisites()?;

    let quote = load_quote_bytes(sealed_data_path)?;
    let parsed_quote = Quote::from_bytes(&quote).ok_or_else(|| {
        TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            "failed to parse SGX quote from configured evidence source",
        )
    })?;

    let outcome = verify_quote_with_dcap(&quote)?;
    enforce_dcap_policy(
        outcome,
        read_bool_env(ENV_SGX_ALLOW_NON_TERMINAL_QV),
        read_bool_env(ENV_SGX_ALLOW_EXPIRED_COLLATERAL),
    )?;

    if let Some(expected) = expected_mrenclave {
        if parsed_quote.mrenclave != expected {
            return Err(TeeError::mrenclave_mismatch(
                &expected,
                &parsed_quote.mrenclave,
            ));
        }
    }
    if let Some(expected) = expected_mrsigner {
        if parsed_quote.mrsigner != expected {
            return Err(TeeError::mrsigner_mismatch(
                &expected,
                &parsed_quote.mrsigner,
            ));
        }
    }
    if parsed_quote.isv_svn < min_isv_svn {
        return Err(TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            format!(
                "SGX quote ISV SVN {} is below required minimum {}",
                parsed_quote.isv_svn, min_isv_svn
            ),
        ));
    }
    if !allow_debug_in_production && (parsed_quote.attributes[0] & 0x02) != 0 {
        return Err(TeeError::enclave_init_error(
            EnclaveInitError::DebugNotAllowed,
            "SGX quote reports debug enclave attributes in strict mode",
        ));
    }

    let sealing_key = load_sealing_key(sealed_data_path)?;
    let expected_key_binding = derive_key_binding_digest(&sealing_key);
    if parsed_quote.report_data[..32] != expected_key_binding {
        return Err(TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            "SGX quote report_data does not match the configured sealing key binding",
        ));
    }

    info!(
        target: "neo::tee",
        isv_prod_id = parsed_quote.isv_prod_id,
        isv_svn = parsed_quote.isv_svn,
        "verified SGX quote and sealing key binding in strict mode"
    );

    Ok(VerifiedSgxRuntimeMaterial {
        evidence: VerifiedSgxEvidence {
            quote,
            mrenclave: parsed_quote.mrenclave,
            mrsigner: parsed_quote.mrsigner,
            isv_prod_id: parsed_quote.isv_prod_id,
            isv_svn: parsed_quote.isv_svn,
            report_data: parsed_quote.report_data,
            cpu_svn: parsed_quote.cpu_svn,
            attributes: parsed_quote.attributes,
        },
        sealing_key,
    })
}

pub(crate) fn verify_quote_signature(quote: &[u8]) -> TeeResult<DcapVerificationOutcome> {
    let outcome = verify_quote_with_dcap(quote)?;
    enforce_dcap_policy(
        outcome,
        read_bool_env(ENV_SGX_ALLOW_NON_TERMINAL_QV),
        read_bool_env(ENV_SGX_ALLOW_EXPIRED_COLLATERAL),
    )?;
    Ok(outcome)
}

pub(crate) fn report_data_from_user_data(user_data: &[u8]) -> [u8; 64] {
    let mut report_data = [0u8; 64];
    let len = user_data.len().min(64);
    report_data[..len].copy_from_slice(&user_data[..len]);
    report_data
}

fn ensure_hardware_prerequisites() -> TeeResult<()> {
    let sgx_enclave = Path::new("/dev/sgx_enclave");
    if !sgx_enclave.exists() {
        return Err(TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            "missing /dev/sgx_enclave device",
        ));
    }

    let provision_nodes = [
        Path::new("/dev/sgx_provision"),
        Path::new("/dev/sgx/provision"),
    ];
    if !provision_nodes.iter().any(|path| path.exists()) {
        return Err(TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            "missing SGX provisioning device (/dev/sgx_provision)",
        ));
    }

    Ok(())
}

fn load_quote_bytes(sealed_data_path: &Path) -> TeeResult<Vec<u8>> {
    if let Ok(hex_quote) = std::env::var(ENV_SGX_QUOTE_HEX) {
        let trimmed = normalize_hex_input(&hex_quote);
        let decoded = hex::decode(trimmed).map_err(|e| {
            TeeError::enclave_init_error(
                EnclaveInitError::HardwareUnavailable,
                format!("{ENV_SGX_QUOTE_HEX} is not valid hex: {e}"),
            )
        })?;
        if decoded.is_empty() {
            return Err(TeeError::enclave_init_error(
                EnclaveInitError::HardwareUnavailable,
                format!("{ENV_SGX_QUOTE_HEX} decoded to an empty quote"),
            ));
        }
        return Ok(decoded);
    }

    let quote_path =
        env_path(ENV_SGX_QUOTE_PATH).unwrap_or_else(|| sealed_data_path.join("sgx.quote"));
    let quote = fs::read(&quote_path).map_err(|e| {
        TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            format!(
                "failed to read SGX quote from {}: {}",
                quote_path.display(),
                e
            ),
        )
    })?;

    if quote.is_empty() {
        return Err(TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            format!("SGX quote file {} is empty", quote_path.display()),
        ));
    }

    Ok(quote)
}

fn load_sealing_key(sealed_data_path: &Path) -> TeeResult<[u8; 32]> {
    if let Ok(hex_key) = std::env::var(ENV_SGX_SEALING_KEY_HEX) {
        return decode_sealing_key_from_hex(&hex_key).map_err(|e| {
            TeeError::enclave_init_error(
                EnclaveInitError::HardwareUnavailable,
                format!("{ENV_SGX_SEALING_KEY_HEX} is invalid: {e}"),
            )
        });
    }

    let key_path = env_path(ENV_SGX_SEALING_KEY_PATH)
        .unwrap_or_else(|| sealed_data_path.join("sgx.sealing_key"));
    let raw = fs::read(&key_path).map_err(|e| {
        TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            format!(
                "failed to read SGX sealing key from {}: {}",
                key_path.display(),
                e
            ),
        )
    })?;

    parse_key_file_contents(&raw).ok_or_else(|| {
        TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            format!(
                "SGX sealing key file {} must be 32 raw bytes or 64-char hex",
                key_path.display()
            ),
        )
    })
}

fn parse_key_file_contents(data: &[u8]) -> Option<[u8; 32]> {
    if data.len() == 32 {
        let mut key = [0u8; 32];
        key.copy_from_slice(data);
        return Some(key);
    }

    let text = std::str::from_utf8(data).ok()?;
    decode_sealing_key_from_hex(text).ok()
}

fn decode_sealing_key_from_hex(input: &str) -> Result<[u8; 32], String> {
    let normalized = normalize_hex_input(input);
    let bytes = hex::decode(normalized).map_err(|e| e.to_string())?;
    if bytes.len() != 32 {
        return Err(format!("expected 32 bytes, got {}", bytes.len()));
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&bytes);
    Ok(key)
}

fn normalize_hex_input(input: &str) -> &str {
    input
        .trim()
        .trim_start_matches("0x")
        .trim_start_matches("0X")
}

fn derive_key_binding_digest(key: &[u8; 32]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(b"neo-tee-sgx-sealing-key-v1");
    hasher.update(key);
    let digest = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&digest);
    output
}

fn verify_quote_with_dcap(quote: &[u8]) -> TeeResult<DcapVerificationOutcome> {
    if quote.len() > u32::MAX as usize {
        return Err(TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            "quote size exceeds DCAP API limit (u32::MAX)",
        ));
    }

    let lib = load_quoteverify_library()?;
    // SAFETY: symbol name and signature are matched against Intel DCAP API.
    let verify_quote: Symbol<SgxQvVerifyQuote> = unsafe {
        lib.get(b"sgx_qv_verify_quote\0").map_err(|e| {
            TeeError::enclave_init_error(
                EnclaveInitError::HardwareUnavailable,
                format!("failed to resolve sgx_qv_verify_quote symbol: {}", e),
            )
        })?
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let expiration_check_date = now.try_into().unwrap_or(c_long::MAX);

    let mut collateral_expiration_status = 1u32;
    let mut quote_verification_result = SGX_QL_QV_RESULT_UNSPECIFIED;

    // SAFETY: pointers are valid for the duration of the call and sizes match buffers.
    let qv_ret = unsafe {
        verify_quote(
            quote.as_ptr(),
            quote.len() as u32,
            std::ptr::null(),
            expiration_check_date,
            &mut collateral_expiration_status as *mut u32,
            &mut quote_verification_result as *mut u32,
            std::ptr::null_mut(),
            0,
            std::ptr::null_mut(),
        )
    };

    if qv_ret != SGX_QL_SUCCESS {
        return Err(TeeError::enclave_init_error(
            EnclaveInitError::HardwareUnavailable,
            format!(
                "sgx_qv_verify_quote failed with status 0x{qv_ret:04x} ({})",
                qv_error_description(qv_ret)
            ),
        ));
    }

    Ok(DcapVerificationOutcome {
        collateral_expiration_status,
        quote_verification_result,
    })
}

fn load_quoteverify_library() -> TeeResult<Library> {
    if let Ok(custom_path) = std::env::var(ENV_SGX_QV_LIB_PATH) {
        // SAFETY: loading a shared object requested by operator configuration.
        let lib = unsafe { Library::new(&custom_path) }.map_err(|e| {
            TeeError::enclave_init_error(
                EnclaveInitError::HardwareUnavailable,
                format!(
                    "failed to load SGX quote verification library from {}: {}",
                    custom_path, e
                ),
            )
        })?;
        return Ok(lib);
    }

    let mut last_error = None;
    for name in SGX_QUOTEVERIFY_LIB_NAMES {
        // SAFETY: loading known Intel DCAP shared library by name.
        match unsafe { Library::new(name) } {
            Ok(lib) => return Ok(lib),
            Err(err) => last_error = Some(err),
        }
    }

    Err(TeeError::enclave_init_error(
        EnclaveInitError::HardwareUnavailable,
        format!(
            "failed to load SGX quote verification library ({:?}): {}",
            SGX_QUOTEVERIFY_LIB_NAMES,
            last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "unknown error".to_string())
        ),
    ))
}

fn enforce_dcap_policy(
    outcome: DcapVerificationOutcome,
    allow_non_terminal_qv: bool,
    allow_expired_collateral: bool,
) -> TeeResult<()> {
    if outcome.collateral_expiration_status != 0 {
        if allow_expired_collateral {
            warn!(
                target: "neo::tee",
                collateral_expiration_status = outcome.collateral_expiration_status,
                "SGX collateral is expired but allowed by configuration override"
            );
        } else {
            return Err(TeeError::enclave_init_error(
                EnclaveInitError::HardwareUnavailable,
                format!(
                    "SGX collateral expired (status={})",
                    outcome.collateral_expiration_status
                ),
            ));
        }
    }

    if outcome.quote_verification_result == SGX_QL_QV_RESULT_OK {
        debug!(target: "neo::tee", "SGX quote verification result=OK");
        return Ok(());
    }

    if allow_non_terminal_qv && is_non_terminal_qv_result(outcome.quote_verification_result) {
        warn!(
            target: "neo::tee",
            qv_result = format_args!("0x{:04x}", outcome.quote_verification_result),
            description = qv_result_description(outcome.quote_verification_result),
            "SGX quote verification produced a non-terminal result allowed by override"
        );
        return Ok(());
    }

    Err(TeeError::enclave_init_error(
        EnclaveInitError::HardwareUnavailable,
        format!(
            "SGX quote verification rejected quote: 0x{:04x} ({})",
            outcome.quote_verification_result,
            qv_result_description(outcome.quote_verification_result)
        ),
    ))
}

fn is_non_terminal_qv_result(result: u32) -> bool {
    matches!(
        result,
        SGX_QL_QV_RESULT_CONFIG_NEEDED
            | SGX_QL_QV_RESULT_OUT_OF_DATE
            | SGX_QL_QV_RESULT_OUT_OF_DATE_CONFIG_NEEDED
            | SGX_QL_QV_RESULT_SW_HARDENING_NEEDED
            | SGX_QL_QV_RESULT_CONFIG_AND_SW_HARDENING_NEEDED
    )
}

fn qv_result_description(result: u32) -> &'static str {
    match result {
        SGX_QL_QV_RESULT_OK => "ok",
        SGX_QL_QV_RESULT_CONFIG_NEEDED => "config needed",
        SGX_QL_QV_RESULT_OUT_OF_DATE => "out of date",
        SGX_QL_QV_RESULT_OUT_OF_DATE_CONFIG_NEEDED => "out of date and config needed",
        SGX_QL_QV_RESULT_INVALID_SIGNATURE => "invalid signature",
        SGX_QL_QV_RESULT_REVOKED => "revoked",
        SGX_QL_QV_RESULT_UNSPECIFIED => "unspecified",
        SGX_QL_QV_RESULT_SW_HARDENING_NEEDED => "software hardening needed",
        SGX_QL_QV_RESULT_CONFIG_AND_SW_HARDENING_NEEDED => {
            "configuration and software hardening needed"
        }
        _ => "unknown",
    }
}

fn qv_error_description(error: u32) -> &'static str {
    match error {
        SGX_QL_SUCCESS => "success",
        0x0000_E01B => "no quote collateral data",
        0x0000_E034 => "unable to get collateral",
        0x0000_E019 => "network error",
        0x0000_E01A => "message error",
        0x0000_E040 => "service unavailable",
        _ => "unknown",
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var(name)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

fn read_bool_env(name: &str) -> bool {
    std::env::var(name)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_report_data_from_user_data_padding() {
        let data = report_data_from_user_data(b"neo");
        assert_eq!(&data[..3], b"neo");
        assert!(data[3..].iter().all(|b| *b == 0));
    }

    #[test]
    fn test_parse_key_file_contents_binary_and_hex() {
        let binary = [0xABu8; 32];
        assert_eq!(parse_key_file_contents(&binary), Some(binary));

        let hex = "ab".repeat(32);
        assert_eq!(parse_key_file_contents(hex.as_bytes()), Some(binary));
    }

    #[test]
    fn test_key_binding_digest_is_deterministic() {
        let key = [0x42u8; 32];
        let digest1 = derive_key_binding_digest(&key);
        let digest2 = derive_key_binding_digest(&key);
        assert_eq!(digest1, digest2);
    }
}
