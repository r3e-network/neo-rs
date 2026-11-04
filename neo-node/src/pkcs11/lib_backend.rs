//! Hand-written minimal PKCS#11 v2.40 FFI bindings and dynamic-library backend.
//!
//! This is the "real" backend: it dlopen-loads the configured PKCS#11 module
//! (the same `libloading` pattern used by the SGX adapter in `neo-tee`) and
//! resolves the smallest function set required by the hardware TEE signer:
//! `C_Initialize` (with `CKF_OS_LOCKING_OK`), slot/token discovery, read-only
//! public-key export, user login, and ECDSA signing. Every failure fails
//! closed; there is no silent fallback to a software signer.

use super::backend::{Pkcs11Backend, Pkcs11KeyQuery, Pkcs11Mechanism, Pkcs11SignCall};
use crate::config::SignerProviderSection;
use anyhow::{Context, Result, bail};
use libloading::{Library, Symbol};
use neo_core::cryptography::{ECCurve, ECPoint};
use std::ffi::c_void;
use std::path::Path;
use std::ptr;

// PKCS#11 defines CK_ULONG as C `unsigned long`: 32-bit on Windows (LLP64)
// and matching the pointer width on the other supported platforms.
#[cfg(windows)]
type CkUlong = u32;
#[cfg(all(not(windows), target_pointer_width = "64"))]
type CkUlong = u64;
#[cfg(all(not(windows), target_pointer_width = "32"))]
type CkUlong = u32;

type CkRv = CkUlong;
type CkSlotId = CkUlong;
type CkSessionHandle = CkUlong;
type CkObjectHandle = CkUlong;
type CkNotify = unsafe extern "C" fn(
    h_session: CkSessionHandle,
    event: CkUlong,
    p_application: *mut c_void,
) -> CkRv;

const CKR_OK: CkRv = 0;
const CK_TRUE: u8 = 1;
const CKF_OS_LOCKING_OK: CkUlong = 0x0000_0002;
const CKF_SERIAL_SESSION: CkUlong = 0x0000_0004;
const CKF_RW_SESSION: CkUlong = 0x0000_0002;
const CKU_USER: CkUlong = 1;
const CKO_PUBLIC_KEY: CkUlong = 0x0000_0002;
const CKO_PRIVATE_KEY: CkUlong = 0x0000_0003;
const CKA_CLASS: CkUlong = 0x0000_0000;
const CKA_LABEL: CkUlong = 0x0000_0003;
const CKA_EC_PARAMS: CkUlong = 0x0000_0180;
const CKA_EC_POINT: CkUlong = 0x0000_0181;

/// `CKA_EC_PARAMS` value for the prime256v1 (secp256r1) named curve.
const PRIME256V1_DER: [u8; 10] = [0x06, 0x08, 0x2A, 0x86, 0x48, 0xCE, 0x3D, 0x03, 0x01, 0x07];

#[repr(C)]
struct CkVersion {
    major: u8,
    minor: u8,
}

#[repr(C)]
struct CkInitializeArgs {
    create_mutex: Option<unsafe extern "C" fn(*mut c_void) -> CkRv>,
    destroy_mutex: Option<unsafe extern "C" fn(*mut c_void) -> CkRv>,
    lock_mutex: Option<unsafe extern "C" fn(*mut c_void) -> CkRv>,
    unlock_mutex: Option<unsafe extern "C" fn(*mut c_void) -> CkRv>,
    flags: CkUlong,
    reserved: *mut c_void,
}

#[repr(C)]
struct CkMechanism {
    mechanism: CkUlong,
    parameter: *mut c_void,
    parameter_len: CkUlong,
}

#[repr(C)]
struct CkAttribute {
    attr_type: CkUlong,
    value: *mut c_void,
    value_len: CkUlong,
}

#[repr(C)]
struct CkTokenInfo {
    label: [u8; 32],
    manufacturer_id: [u8; 32],
    model: [u8; 16],
    serial_number: [u8; 16],
    flags: CkUlong,
    max_session_count: CkUlong,
    session_count: CkUlong,
    max_rw_session_count: CkUlong,
    rw_session_count: CkUlong,
    max_pin_len: CkUlong,
    min_pin_len: CkUlong,
    total_public_memory: CkUlong,
    free_public_memory: CkUlong,
    total_private_memory: CkUlong,
    free_private_memory: CkUlong,
    hardware_version: CkVersion,
    firmware_version: CkVersion,
    utc_time: [u8; 16],
}

type CInitializeFn = unsafe extern "C" fn(p_init_args: *mut CkInitializeArgs) -> CkRv;
type CFinalizeFn = unsafe extern "C" fn(p_reserved: *mut c_void) -> CkRv;
type CGetSlotListFn =
    unsafe extern "C" fn(token_present: u8, slot_list: *mut CkSlotId, count: *mut CkUlong) -> CkRv;
type CGetTokenInfoFn = unsafe extern "C" fn(slot_id: CkSlotId, info: *mut CkTokenInfo) -> CkRv;
type CGetMechanismListFn = unsafe extern "C" fn(
    slot_id: CkSlotId,
    mechanism_list: *mut CkUlong,
    count: *mut CkUlong,
) -> CkRv;
type COpenSessionFn = unsafe extern "C" fn(
    slot_id: CkSlotId,
    flags: CkUlong,
    application: *mut c_void,
    notify: Option<CkNotify>,
    session: *mut CkSessionHandle,
) -> CkRv;
type CCloseSessionFn = unsafe extern "C" fn(session: CkSessionHandle) -> CkRv;
type CLoginFn = unsafe extern "C" fn(
    session: CkSessionHandle,
    user_type: CkUlong,
    pin: *const u8,
    pin_len: CkUlong,
) -> CkRv;
type CFindObjectsInitFn = unsafe extern "C" fn(
    session: CkSessionHandle,
    template: *const CkAttribute,
    count: CkUlong,
) -> CkRv;
type CFindObjectsFn = unsafe extern "C" fn(
    session: CkSessionHandle,
    objects: *mut CkObjectHandle,
    max_count: CkUlong,
    actual_count: *mut CkUlong,
) -> CkRv;
type CFindObjectsFinalFn = unsafe extern "C" fn(session: CkSessionHandle) -> CkRv;
type CGetAttributeValueFn = unsafe extern "C" fn(
    session: CkSessionHandle,
    object: CkObjectHandle,
    template: *mut CkAttribute,
    count: CkUlong,
) -> CkRv;
type CSignInitFn = unsafe extern "C" fn(
    session: CkSessionHandle,
    mechanism: *mut CkMechanism,
    key: CkObjectHandle,
) -> CkRv;
type CSignFn = unsafe extern "C" fn(
    session: CkSessionHandle,
    data: *const u8,
    data_len: CkUlong,
    signature: *mut u8,
    signature_len: *mut CkUlong,
) -> CkRv;

/// Resolved PKCS#11 entry points. Raw pointers are valid for as long as the
/// owning [`Library`] stays loaded, which `LibPkcs11Backend` guarantees.
struct CkFunctions {
    c_initialize: CInitializeFn,
    c_finalize: CFinalizeFn,
    c_get_slot_list: CGetSlotListFn,
    c_get_token_info: CGetTokenInfoFn,
    c_get_mechanism_list: CGetMechanismListFn,
    c_open_session: COpenSessionFn,
    c_close_session: CCloseSessionFn,
    c_login: CLoginFn,
    c_find_objects_init: CFindObjectsInitFn,
    c_find_objects: CFindObjectsFn,
    c_find_objects_final: CFindObjectsFinalFn,
    c_get_attribute_value: CGetAttributeValueFn,
    c_sign_init: CSignInitFn,
    c_sign: CSignFn,
}

impl CkFunctions {
    /// Resolves the minimal entry-point set from a loaded PKCS#11 library.
    ///
    /// # Safety
    ///
    /// `library` must be a dynamically loaded PKCS#11 v2.40 module and stay
    /// loaded for the lifetime of the returned function pointers.
    unsafe fn load(library: &Library) -> Result<Self> {
        // SAFETY: all names below are the standard PKCS#11 v2.40 entry points
        // and the library owns the resolved pointers.
        unsafe fn symbol<T: Copy>(library: &Library, name: &[u8]) -> Result<T> {
            let resolved: Symbol<T> = unsafe { library.get(name) }.with_context(|| {
                format!(
                    "PKCS#11 library is missing '{}'",
                    String::from_utf8_lossy(&name[..name.len() - 1])
                )
            })?;
            Ok(*resolved)
        }
        Ok(Self {
            c_initialize: unsafe { symbol(library, b"C_Initialize\0")? },
            c_finalize: unsafe { symbol(library, b"C_Finalize\0")? },
            c_get_slot_list: unsafe { symbol(library, b"C_GetSlotList\0")? },
            c_get_token_info: unsafe { symbol(library, b"C_GetTokenInfo\0")? },
            c_get_mechanism_list: unsafe { symbol(library, b"C_GetMechanismList\0")? },
            c_open_session: unsafe { symbol(library, b"C_OpenSession\0")? },
            c_close_session: unsafe { symbol(library, b"C_CloseSession\0")? },
            c_login: unsafe { symbol(library, b"C_Login\0")? },
            c_find_objects_init: unsafe { symbol(library, b"C_FindObjectsInit\0")? },
            c_find_objects: unsafe { symbol(library, b"C_FindObjects\0")? },
            c_find_objects_final: unsafe { symbol(library, b"C_FindObjectsFinal\0")? },
            c_get_attribute_value: unsafe { symbol(library, b"C_GetAttributeValue\0")? },
            c_sign_init: unsafe { symbol(library, b"C_SignInit\0")? },
            c_sign: unsafe { symbol(library, b"C_Sign\0")? },
        })
    }
}

fn check_rv(rv: CkRv, operation: &str) -> Result<()> {
    if rv == CKR_OK {
        Ok(())
    } else {
        bail!("PKCS#11 {operation} failed with return code 0x{rv:08x}")
    }
}

/// Trims a fixed-size, space- or NUL-padded PKCS#11 character field.
fn trim_ck_string(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end])
        .trim_end()
        .to_string()
}

/// Closes the session when the guard is dropped.
struct SessionGuard<'a> {
    functions: &'a CkFunctions,
    handle: CkSessionHandle,
}

impl Drop for SessionGuard<'_> {
    fn drop(&mut self) {
        // SAFETY: the handle was produced by `C_OpenSession` on the same
        // library and is dropped exactly once before `C_Finalize`.
        let _ = unsafe { (self.functions.c_close_session)(self.handle) };
    }
}

/// Real PKCS#11 backend over a dynamically loaded module.
///
/// `Send`/`Sync` safety: the library is initialized with
/// `CKF_OS_LOCKING_OK`, under which the PKCS#11 v2.40 specification
/// (section 2.2.2) requires the module to protect its own state with OS
/// locking primitives, making its entry points thread-safe. `C_Finalize`
/// runs only in `Drop`, when no references to the backend can remain.
pub(crate) struct LibPkcs11Backend {
    /// Keep-alive handle for the loaded module; deliberately never read. The
    /// resolved `CkFunctions` pointers stay valid only while the library is
    /// loaded, and drop order keeps this alive until the backend is dropped
    /// (after `C_Finalize` has run in `Drop`).
    _library: Library,
    functions: CkFunctions,
}

// SAFETY: see the struct-level comment; all calls are serialized by the
// module itself through `CKF_OS_LOCKING_OK` OS locking.
unsafe impl Send for LibPkcs11Backend {}
// SAFETY: see the struct-level comment; all calls are serialized by the
// module itself through `CKF_OS_LOCKING_OK` OS locking.
unsafe impl Sync for LibPkcs11Backend {}

impl LibPkcs11Backend {
    /// Loads and initializes a PKCS#11 module from `library_path`.
    pub(crate) fn load(library_path: &Path) -> Result<Self> {
        let library = unsafe { Library::new(library_path) }.with_context(|| {
            format!(
                "failed to load the PKCS#11 library '{}'",
                library_path.display()
            )
        })?;
        // SAFETY: `library` stays owned by the returned backend.
        let functions = unsafe { CkFunctions::load(&library) }?;
        let mut args = CkInitializeArgs {
            create_mutex: None,
            destroy_mutex: None,
            lock_mutex: None,
            unlock_mutex: None,
            flags: CKF_OS_LOCKING_OK,
            reserved: ptr::null_mut(),
        };
        // SAFETY: `args` is a stack-local CK_C_INITIALIZE_ARGS enabling OS
        // locking, as required for cross-thread use.
        let rv = unsafe { (functions.c_initialize)(&mut args) };
        check_rv(rv, "C_Initialize")
            .with_context(|| format!("failed to initialize '{}'", library_path.display()))?;
        Ok(Self {
            _library: library,
            functions,
        })
    }

    /// Loads the backend declared by a hardware TEE provider section.
    pub(crate) fn from_provider_section(provider: &SignerProviderSection) -> Result<Self> {
        let pkcs11 = provider.pkcs11.as_ref().with_context(|| {
            format!(
                "hardware TEE signer provider '{}' requires a pkcs11 configuration section",
                provider.id
            )
        })?;
        Self::load(Path::new(&pkcs11.library_path))
    }

    fn slot_ids(&self) -> Result<Vec<CkSlotId>> {
        let mut count: CkUlong = 0;
        // SAFETY: `count` is a valid out-pointer and the list pointer is null
        // for the size-query call.
        let rv = unsafe { (self.functions.c_get_slot_list)(CK_TRUE, ptr::null_mut(), &mut count) };
        check_rv(rv, "C_GetSlotList")?;
        let mut slots = vec![0 as CkSlotId; count.max(1) as usize];
        // SAFETY: `slots` has `count` entries and `count` is reset by the call.
        let rv =
            unsafe { (self.functions.c_get_slot_list)(CK_TRUE, slots.as_mut_ptr(), &mut count) };
        check_rv(rv, "C_GetSlotList")?;
        slots.truncate(count as usize);
        Ok(slots)
    }

    fn require_slot(&self, slot_id: u64) -> Result<CkSlotId> {
        let slot = CkSlotId::try_from(slot_id)
            .map_err(|_| anyhow::anyhow!("PKCS#11 slot id {slot_id} is out of range"))?;
        if !self.slot_ids()?.contains(&slot) {
            bail!("PKCS#11 slot {slot_id} has no token present");
        }
        Ok(slot)
    }

    fn verify_token(&self, slot: CkSlotId, token_label: &str) -> Result<()> {
        let mut info = CkTokenInfo {
            label: [0; 32],
            manufacturer_id: [0; 32],
            model: [0; 16],
            serial_number: [0; 16],
            flags: 0,
            max_session_count: 0,
            session_count: 0,
            max_rw_session_count: 0,
            rw_session_count: 0,
            max_pin_len: 0,
            min_pin_len: 0,
            total_public_memory: 0,
            free_public_memory: 0,
            total_private_memory: 0,
            free_private_memory: 0,
            hardware_version: CkVersion { major: 0, minor: 0 },
            firmware_version: CkVersion { major: 0, minor: 0 },
            utc_time: [0; 16],
        };
        // SAFETY: `info` is a fully initialized CK_TOKEN_INFO out-parameter.
        let rv = unsafe { (self.functions.c_get_token_info)(slot, &mut info) };
        check_rv(rv, "C_GetTokenInfo")?;
        let label = trim_ck_string(&info.label);
        if label != token_label {
            bail!(
                "PKCS#11 token label '{label}' does not match the configured label '{token_label}'"
            );
        }
        Ok(())
    }

    fn open_session(&self, slot: CkSlotId, read_write: bool) -> Result<SessionGuard<'_>> {
        let flags = if read_write {
            CKF_SERIAL_SESSION | CKF_RW_SESSION
        } else {
            CKF_SERIAL_SESSION
        };
        let mut handle: CkSessionHandle = 0;
        // SAFETY: `handle` is a valid out-pointer and no application callback
        // is registered.
        let rv = unsafe {
            (self.functions.c_open_session)(slot, flags, ptr::null_mut(), None, &mut handle)
        };
        check_rv(rv, "C_OpenSession")?;
        Ok(SessionGuard {
            functions: &self.functions,
            handle,
        })
    }

    fn find_object(
        &self,
        session: CkSessionHandle,
        object_class: CkUlong,
        key_label: &str,
    ) -> Result<CkObjectHandle> {
        let mut class_value = object_class;
        let label_bytes = key_label.as_bytes();
        let template = [
            CkAttribute {
                attr_type: CKA_CLASS,
                value: &mut class_value as *mut CkUlong as *mut c_void,
                value_len: std::mem::size_of::<CkUlong>() as CkUlong,
            },
            CkAttribute {
                attr_type: CKA_LABEL,
                value: label_bytes.as_ptr() as *mut c_void,
                value_len: label_bytes.len() as CkUlong,
            },
        ];
        // SAFETY: `template` is a two-entry CK_ATTRIBUTE array whose values
        // outlive the find operation.
        let rv = unsafe {
            (self.functions.c_find_objects_init)(
                session,
                template.as_ptr(),
                template.len() as CkUlong,
            )
        };
        check_rv(rv, "C_FindObjectsInit")?;
        let mut handle: CkObjectHandle = 0;
        let mut found: CkUlong = 0;
        // SAFETY: `handle` and `found` are valid out-pointers.
        let rv = unsafe { (self.functions.c_find_objects)(session, &mut handle, 1, &mut found) };
        check_rv(rv, "C_FindObjects")?;
        // SAFETY: the search operation has completed above.
        let rv = unsafe { (self.functions.c_find_objects_final)(session) };
        check_rv(rv, "C_FindObjectsFinal")?;
        if found == 0 {
            bail!("PKCS#11 token has no key object with label '{key_label}'");
        }
        Ok(handle)
    }

    fn read_attribute(
        &self,
        session: CkSessionHandle,
        object: CkObjectHandle,
        attr_type: CkUlong,
    ) -> Result<Vec<u8>> {
        let mut attribute = CkAttribute {
            attr_type,
            value: ptr::null_mut(),
            value_len: 0,
        };
        // SAFETY: the null value pointer performs the size query.
        let rv =
            unsafe { (self.functions.c_get_attribute_value)(session, object, &mut attribute, 1) };
        check_rv(rv, "C_GetAttributeValue")?;
        let mut buffer = vec![0u8; attribute.value_len.max(1) as usize];
        attribute.value = buffer.as_mut_ptr() as *mut c_void;
        // SAFETY: `buffer` matches the queried length and `attribute` is reset.
        let rv =
            unsafe { (self.functions.c_get_attribute_value)(session, object, &mut attribute, 1) };
        check_rv(rv, "C_GetAttributeValue")?;
        buffer.truncate(attribute.value_len as usize);
        Ok(buffer)
    }

    fn mechanism_codes(&self, slot: CkSlotId) -> Vec<CkUlong> {
        let mut count: CkUlong = 0;
        // SAFETY: size-query call with a null mechanism list.
        let rv =
            unsafe { (self.functions.c_get_mechanism_list)(slot, ptr::null_mut(), &mut count) };
        if rv != CKR_OK || count == 0 {
            return Vec::new();
        }
        let mut codes = vec![0 as CkUlong; count as usize];
        // SAFETY: `codes` has `count` entries and `count` is reset by the call.
        let rv =
            unsafe { (self.functions.c_get_mechanism_list)(slot, codes.as_mut_ptr(), &mut count) };
        if rv != CKR_OK {
            return Vec::new();
        }
        codes.truncate(count as usize);
        codes
    }
}

impl Drop for LibPkcs11Backend {
    fn drop(&mut self) {
        // SAFETY: `C_Finalize` must run before the library is unloaded and
        // `Drop` only runs when every shared reference is gone.
        let _ = unsafe { (self.functions.c_finalize)(ptr::null_mut()) };
    }
}

/// Decodes a `CKA_EC_POINT` DER OCTET STRING into compressed SEC1 bytes.
fn decode_ec_point(raw: &[u8]) -> Result<Vec<u8>> {
    if raw.len() < 3 || raw[0] != 0x04 {
        bail!("CKA_EC_POINT is not a DER OCTET STRING");
    }
    let length = raw[1] as usize;
    if length > 0x7f || raw.len() != length + 2 {
        bail!("CKA_EC_POINT has an unsupported DER length");
    }
    let point = ECPoint::new(ECCurve::Secp256r1, raw[2..].to_vec())
        .map_err(|error| anyhow::anyhow!("HSM EC point is not a valid P-256 point: {error}"))?;
    Ok(point.to_bytes())
}

impl Pkcs11Backend for LibPkcs11Backend {
    fn supports_mechanism(&self, slot_id: u64, mechanism: Pkcs11Mechanism) -> bool {
        let Ok(slot) = self.require_slot(slot_id) else {
            return false;
        };
        self.mechanism_codes(slot)
            .contains(&(mechanism.raw_code() as CkUlong))
    }

    fn export_public_key(&self, query: &Pkcs11KeyQuery) -> Result<Vec<u8>> {
        let slot = self.require_slot(query.slot_id)?;
        self.verify_token(slot, &query.token_label)?;
        let session = self.open_session(slot, false)?;
        let object = self.find_object(session.handle, CKO_PUBLIC_KEY, &query.key_label)?;
        let params = self.read_attribute(session.handle, object, CKA_EC_PARAMS)?;
        if params != PRIME256V1_DER {
            bail!(
                "HSM key '{}' is not a prime256v1 (secp256r1) key",
                query.key_label
            );
        }
        let point = self.read_attribute(session.handle, object, CKA_EC_POINT)?;
        decode_ec_point(&point)
    }

    fn sign(&self, call: &Pkcs11SignCall) -> Result<Vec<u8>> {
        let slot = self.require_slot(call.slot_id)?;
        self.verify_token(slot, &call.token_label)?;
        let session = self.open_session(slot, true)?;
        // SAFETY: `call.pin` is a valid PIN buffer for the duration of the login.
        let rv = unsafe {
            (self.functions.c_login)(
                session.handle,
                CKU_USER,
                call.pin.as_ptr(),
                call.pin.len() as CkUlong,
            )
        };
        check_rv(rv, "C_Login").context("HSM user login failed (verify NEO_HSM_PIN)")?;
        let key = self.find_object(session.handle, CKO_PRIVATE_KEY, &call.key_label)?;
        let mut mechanism = CkMechanism {
            mechanism: call.mechanism.raw_code() as CkUlong,
            parameter: ptr::null_mut(),
            parameter_len: 0,
        };
        // SAFETY: `mechanism` is a stack-local CK_MECHANISM without parameters.
        let rv = unsafe { (self.functions.c_sign_init)(session.handle, &mut mechanism, key) };
        check_rv(rv, "C_SignInit")?;
        let mut signature_len: CkUlong = 0;
        // SAFETY: the null signature buffer performs the size query.
        let rv = unsafe {
            (self.functions.c_sign)(
                session.handle,
                call.data.as_ptr(),
                call.data.len() as CkUlong,
                ptr::null_mut(),
                &mut signature_len,
            )
        };
        check_rv(rv, "C_Sign")?;
        if signature_len != 64 {
            bail!(
                "PKCS#11 C_Sign reported an unexpected signature length {signature_len}; raw r||s ECDSA is required"
            );
        }
        let mut signature = vec![0u8; 64];
        // SAFETY: `signature` matches the queried length of 64 bytes.
        let rv = unsafe {
            (self.functions.c_sign)(
                session.handle,
                call.data.as_ptr(),
                call.data.len() as CkUlong,
                signature.as_mut_ptr(),
                &mut signature_len,
            )
        };
        check_rv(rv, "C_Sign")?;
        Ok(signature)
    }
}
