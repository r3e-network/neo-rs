//! PKCS#11 HSM signer implementation

use crate::device::{HsmDeviceInfo, HsmDeviceType};
use crate::error::{HsmError, HsmResult};
use crate::signer::{HsmKeyInfo, HsmSigner};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::path::Path;

use cryptoki::{
    context::{CInitializeArgs, Pkcs11},
    mechanism::Mechanism,
    object::{Attribute, AttributeType, ObjectClass, ObjectHandle},
    session::{Session, UserType},
    types::AuthPin,
};

/// PKCS#11 HSM signer
pub struct Pkcs11Signer {
    device_info: HsmDeviceInfo,
    ctx: Pkcs11,
    slot_index: u64,
    session: RwLock<Option<Session>>,
    is_ready: RwLock<bool>,
    is_locked: RwLock<bool>,
}

impl Pkcs11Signer {
    /// Create a new PKCS#11 signer
    ///
    /// # Arguments
    /// * `library_path` - Path to the PKCS#11 library (.so/.dll)
    /// * `slot` - Slot index to use
    pub fn new(library_path: &Path, slot: u64) -> HsmResult<Self> {
        let ctx = Pkcs11::new(library_path)
            .map_err(|e| HsmError::Pkcs11Error(format!("Failed to load library: {}", e)))?;

        ctx.initialize(CInitializeArgs::OsThreads)
            .map_err(|e| HsmError::Pkcs11Error(format!("Failed to initialize: {}", e)))?;

        let slots = ctx
            .get_slots_with_token()
            .map_err(|e| HsmError::Pkcs11Error(format!("Failed to get slots: {}", e)))?;

        let slot_obj = slots.get(slot as usize).ok_or_else(|| {
            HsmError::DeviceNotFound(format!(
                "Slot {} not found. {} slot(s) available.",
                slot,
                slots.len()
            ))
        })?;

        let token_info = ctx
            .get_token_info(*slot_obj)
            .map_err(|e| HsmError::Pkcs11Error(format!("Failed to get token info: {}", e)))?;

        let device_info = HsmDeviceInfo {
            device_type: HsmDeviceType::Pkcs11,
            manufacturer: token_info.manufacturer_id().trim().to_string(),
            model: token_info.model().trim().to_string(),
            serial_number: Some(token_info.serial_number().trim().to_string()),
            firmware_version: Some(format!(
                "{}.{}",
                token_info.firmware_version().major(),
                token_info.firmware_version().minor()
            )),
            is_connected: true,
            requires_pin: true,
        };

        Ok(Self {
            device_info,
            ctx,
            slot_index: slot,
            session: RwLock::new(None),
            is_ready: RwLock::new(false),
            is_locked: RwLock::new(true),
        })
    }

    /// Get the slot object for the configured slot index
    fn get_slot(&self) -> HsmResult<cryptoki::slot::Slot> {
        let slots = self
            .ctx
            .get_slots_with_token()
            .map_err(|e| HsmError::Pkcs11Error(e.to_string()))?;

        slots
            .get(self.slot_index as usize)
            .copied()
            .ok_or_else(|| HsmError::DeviceNotFound(format!("Slot {}", self.slot_index)))
    }

    /// Find a private key by label or ID
    fn find_private_key(&self, session: &Session, key_id: &str) -> HsmResult<ObjectHandle> {
        // Try to find by label first
        let template = vec![
            Attribute::Class(ObjectClass::PRIVATE_KEY),
            Attribute::Label(key_id.as_bytes().to_vec()),
        ];

        let objects = session
            .find_objects(&template)
            .map_err(|e| HsmError::Pkcs11Error(e.to_string()))?;

        if let Some(obj) = objects.first() {
            return Ok(*obj);
        }

        // Try to find by ID (hex-encoded)
        if let Ok(id_bytes) = hex::decode(key_id) {
            let template = vec![
                Attribute::Class(ObjectClass::PRIVATE_KEY),
                Attribute::Id(id_bytes),
            ];

            let objects = session
                .find_objects(&template)
                .map_err(|e| HsmError::Pkcs11Error(e.to_string()))?;

            if let Some(obj) = objects.first() {
                return Ok(*obj);
            }
        }

        Err(HsmError::KeyNotFound(key_id.to_string()))
    }

    /// Find the public key corresponding to a private key
    fn find_public_key(&self, session: &Session, key_id: &str) -> HsmResult<ObjectHandle> {
        let template = vec![
            Attribute::Class(ObjectClass::PUBLIC_KEY),
            Attribute::Label(key_id.as_bytes().to_vec()),
        ];

        let objects = session
            .find_objects(&template)
            .map_err(|e| HsmError::Pkcs11Error(e.to_string()))?;

        if let Some(obj) = objects.first() {
            return Ok(*obj);
        }

        // Try by ID
        if let Ok(id_bytes) = hex::decode(key_id) {
            let template = vec![
                Attribute::Class(ObjectClass::PUBLIC_KEY),
                Attribute::Id(id_bytes),
            ];

            let objects = session
                .find_objects(&template)
                .map_err(|e| HsmError::Pkcs11Error(e.to_string()))?;

            if let Some(obj) = objects.first() {
                return Ok(*obj);
            }
        }

        Err(HsmError::KeyNotFound(key_id.to_string()))
    }

    /// Get key info from a key handle
    fn get_key_info_from_handle(
        &self,
        session: &Session,
        handle: ObjectHandle,
    ) -> HsmResult<HsmKeyInfo> {
        let attrs = session
            .get_attributes(
                handle,
                &[
                    AttributeType::Label,
                    AttributeType::Id,
                    AttributeType::EcPoint,
                ],
            )
            .map_err(|e| HsmError::Pkcs11Error(e.to_string()))?;

        let mut label: Option<String> = None;
        let mut key_id: Option<String> = None;
        let mut public_key: Option<Vec<u8>> = None;

        for attr in attrs {
            match attr {
                Attribute::Label(l) => {
                    label = Some(String::from_utf8_lossy(&l).to_string());
                }
                Attribute::Id(id) => {
                    key_id = Some(hex::encode(&id));
                }
                Attribute::EcPoint(point) => {
                    // EC point is DER-encoded, extract the raw point
                    public_key = Some(self.extract_ec_point(&point)?);
                }
                _ => {}
            }
        }

        let key_id = key_id.or_else(|| label.clone()).ok_or_else(|| {
            HsmError::Pkcs11Error("Key has no label or ID".to_string())
        })?;

        let public_key = public_key.ok_or_else(|| {
            HsmError::Pkcs11Error("Could not get public key".to_string())
        })?;

        // Calculate script hash
        let script_hash_vec = neo_crypto::Crypto::hash160(&public_key);
        let mut script_hash = [0u8; 20];
        script_hash.copy_from_slice(&script_hash_vec);

        let mut info = HsmKeyInfo::new(key_id, public_key, script_hash);
        if let Some(l) = label {
            info = info.with_label(l);
        }

        Ok(info)
    }

    /// Extract raw EC point from DER-encoded EC point
    fn extract_ec_point(&self, der: &[u8]) -> HsmResult<Vec<u8>> {
        // DER format: 0x04 len point...
        // For secp256r1, uncompressed point is 65 bytes (0x04 + 32 + 32)
        // Compressed point is 33 bytes (0x02/0x03 + 32)
        if der.len() < 2 {
            return Err(HsmError::InvalidKeyFormat("EC point too short".to_string()));
        }

        // Skip DER OCTET STRING wrapper if present
        let point = if der[0] == 0x04 && der.len() == 67 {
            // DER: 0x04 0x41 <65-byte uncompressed point>
            &der[2..]
        } else if der[0] == 0x04 && der.len() == 65 {
            // Raw uncompressed point
            der
        } else if (der[0] == 0x02 || der[0] == 0x03) && der.len() == 33 {
            // Already compressed
            return Ok(der.to_vec());
        } else {
            der
        };

        // Compress the point if uncompressed
        if point.len() == 65 && point[0] == 0x04 {
            let x = &point[1..33];
            let y_last = point[64];
            let prefix = if y_last % 2 == 0 { 0x02 } else { 0x03 };
            let mut compressed = vec![prefix];
            compressed.extend_from_slice(x);
            Ok(compressed)
        } else {
            Ok(point.to_vec())
        }
    }
}

#[async_trait]
impl HsmSigner for Pkcs11Signer {
    fn device_info(&self) -> &HsmDeviceInfo {
        &self.device_info
    }

    fn is_ready(&self) -> bool {
        *self.is_ready.read()
    }

    async fn unlock(&self, pin: &str) -> HsmResult<()> {
        let slot = self.get_slot()?;

        let session = self
            .ctx
            .open_rw_session(slot)
            .map_err(|e| HsmError::Pkcs11Error(format!("Failed to open session: {}", e)))?;

        let auth_pin = AuthPin::new(pin.to_string());
        session.login(UserType::User, Some(&auth_pin)).map_err(|e| {
            let err_str = e.to_string();
            if err_str.contains("PIN_INCORRECT") || err_str.contains("CKR_PIN_INCORRECT") {
                HsmError::InvalidPin
            } else if err_str.contains("PIN_LOCKED") || err_str.contains("CKR_PIN_LOCKED") {
                HsmError::PinLocked
            } else {
                HsmError::Pkcs11Error(format!("Login failed: {}", e))
            }
        })?;

        *self.session.write() = Some(session);
        *self.is_locked.write() = false;
        *self.is_ready.write() = true;

        tracing::info!(
            target: "neo::hsm",
            "PKCS#11 device ready: {} {}",
            self.device_info.manufacturer,
            self.device_info.model
        );

        Ok(())
    }

    fn lock(&self) {
        if let Some(session) = self.session.write().take() {
            let _ = session.logout();
        }
        *self.is_locked.write() = true;
        *self.is_ready.write() = false;
    }

    fn is_locked(&self) -> bool {
        *self.is_locked.read()
    }

    async fn list_keys(&self) -> HsmResult<Vec<HsmKeyInfo>> {
        let session_guard = self.session.read();
        let session = session_guard
            .as_ref()
            .ok_or(HsmError::NotInitialized)?;

        // Find all EC public keys (easier to enumerate than private keys)
        let template = vec![Attribute::Class(ObjectClass::PUBLIC_KEY)];

        let objects = session
            .find_objects(&template)
            .map_err(|e| HsmError::Pkcs11Error(e.to_string()))?;

        let mut keys = Vec::new();
        for obj in objects {
            if let Ok(key_info) = self.get_key_info_from_handle(session, obj) {
                keys.push(key_info);
            }
        }

        Ok(keys)
    }

    async fn get_key(&self, key_id: &str) -> HsmResult<HsmKeyInfo> {
        let session_guard = self.session.read();
        let session = session_guard
            .as_ref()
            .ok_or(HsmError::NotInitialized)?;

        let handle = self.find_public_key(session, key_id)?;
        self.get_key_info_from_handle(session, handle)
    }

    async fn sign(&self, key_id: &str, data: &[u8]) -> HsmResult<Vec<u8>> {
        if self.is_locked() {
            return Err(HsmError::PinRequired);
        }

        let session_guard = self.session.read();
        let session = session_guard
            .as_ref()
            .ok_or(HsmError::NotInitialized)?;

        let key_handle = self.find_private_key(session, key_id)?;

        // Use ECDSA mechanism for secp256r1
        let mechanism = Mechanism::Ecdsa;

        let signature = session
            .sign(&mechanism, key_handle, data)
            .map_err(|e| HsmError::SigningFailed(e.to_string()))?;

        Ok(signature)
    }

    async fn get_public_key(&self, key_id: &str) -> HsmResult<Vec<u8>> {
        let key = self.get_key(key_id).await?;
        Ok(key.public_key)
    }

    async fn verify_device(&self) -> HsmResult<bool> {
        Ok(self.is_ready())
    }
}
