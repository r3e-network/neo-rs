//! Ledger hardware wallet signer implementation

use crate::device::{HsmDeviceInfo, HsmDeviceType};
use crate::error::{HsmError, HsmResult};
use crate::signer::{HsmKeyInfo, HsmSigner, normalize_public_key, script_hash_from_public_key};
use async_trait::async_trait;
use parking_lot::RwLock;

use hidapi::HidApi;

/// Ledger USB Vendor ID
const LEDGER_VENDOR_ID: u16 = 0x2c97;

/// Neo Ledger app CLA byte
const NEO_CLA: u8 = 0x80;

/// Ledger APDU instructions for Neo app
#[repr(u8)]
#[derive(Debug, Clone, Copy)]
enum NeoInstruction {
    GetVersion = 0x00,
    GetPublicKey = 0x04,
    SignTransaction = 0x08,
}

/// Ledger hardware wallet signer
pub struct LedgerSigner {
    device_info: HsmDeviceInfo,
    hid_api: HidApi,
    device_index: u32,
    is_ready: RwLock<bool>,
    is_locked: RwLock<bool>,
    cached_keys: RwLock<Vec<HsmKeyInfo>>,
}

impl LedgerSigner {
    /// Create a new Ledger signer
    ///
    /// # Arguments
    /// * `device_index` - Index of the Ledger device (0 for first device)
    pub fn new(device_index: u32) -> HsmResult<Self> {
        let hid_api = HidApi::new()
            .map_err(|e| HsmError::InitFailed(format!("Failed to init HID: {}", e)))?;

        let device_info = Self::find_ledger_device(&hid_api, device_index)?;

        Ok(Self {
            device_info,
            hid_api,
            device_index,
            is_ready: RwLock::new(false),
            is_locked: RwLock::new(true),
            cached_keys: RwLock::new(Vec::new()),
        })
    }

    /// Find a Ledger device by index
    fn find_ledger_device(hid_api: &HidApi, index: u32) -> HsmResult<HsmDeviceInfo> {
        let devices: Vec<_> = hid_api
            .device_list()
            .filter(|d| d.vendor_id() == LEDGER_VENDOR_ID)
            .collect();

        if devices.is_empty() {
            return Err(HsmError::DeviceNotFound(
                "No Ledger device found. Please connect your Ledger and unlock it.".to_string(),
            ));
        }

        let device = devices.get(index as usize).ok_or_else(|| {
            HsmError::DeviceNotFound(format!(
                "Ledger device index {} not found. {} device(s) available.",
                index,
                devices.len()
            ))
        })?;

        Ok(HsmDeviceInfo {
            device_type: HsmDeviceType::Ledger,
            manufacturer: device
                .manufacturer_string()
                .unwrap_or(Some("Ledger"))
                .unwrap_or("Ledger")
                .to_string(),
            model: device
                .product_string()
                .unwrap_or(Some("Unknown"))
                .unwrap_or("Unknown")
                .to_string(),
            serial_number: device.serial_number().map(|s| s.to_string()),
            firmware_version: None,
            is_connected: true,
            requires_pin: true,
        })
    }

    /// Send an APDU command to the Ledger
    fn send_apdu(&self, cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> HsmResult<Vec<u8>> {
        let devices: Vec<_> = self
            .hid_api
            .device_list()
            .filter(|d| d.vendor_id() == LEDGER_VENDOR_ID)
            .collect();

        let device_info = devices
            .get(self.device_index as usize)
            .ok_or_else(|| HsmError::DeviceNotFound("Ledger device disconnected".to_string()))?;

        let device = device_info
            .open_device(&self.hid_api)
            .map_err(|e| HsmError::DeviceError(format!("Failed to open device: {}", e)))?;

        // Build APDU
        let mut apdu = vec![cla, ins, p1, p2];
        if !data.is_empty() {
            apdu.push(data.len() as u8);
            apdu.extend_from_slice(data);
        }

        // Wrap in HID report (Ledger uses 64-byte reports)
        let mut report = vec![0x00]; // Report ID
        report.push(0x01); // Channel
        report.push(0x01);
        report.push(0x05); // Tag
        report.push(0x00); // Sequence high
        report.push(0x00); // Sequence low
        report.push((apdu.len() >> 8) as u8);
        report.push((apdu.len() & 0xff) as u8);
        report.extend_from_slice(&apdu);
        report.resize(65, 0x00);

        // Send
        device
            .write(&report)
            .map_err(|e| HsmError::DeviceError(format!("Write failed: {}", e)))?;

        // Read response
        let mut response = vec![0u8; 65];
        let len = device
            .read_timeout(&mut response, 30000)
            .map_err(|e| HsmError::DeviceError(format!("Read failed: {}", e)))?;

        if len < 9 {
            return Err(HsmError::DeviceError("Invalid response length".to_string()));
        }

        // Parse response (skip HID header)
        let data_len = ((response[5] as usize) << 8) | (response[6] as usize);
        let data = response[7..7 + data_len.min(response.len() - 7)].to_vec();

        // Check status word (last 2 bytes)
        if data.len() >= 2 {
            let sw = ((data[data.len() - 2] as u16) << 8) | (data[data.len() - 1] as u16);
            match sw {
                0x9000 => Ok(data[..data.len() - 2].to_vec()),
                0x6985 => Err(HsmError::UserRejected),
                0x6982 => Err(HsmError::PinLocked),
                0x6700 => Err(HsmError::LedgerError("Invalid data length".to_string())),
                0x6E00 => Err(HsmError::LedgerError("Neo app not open".to_string())),
                _ => Err(HsmError::LedgerError(format!("Status: 0x{:04X}", sw))),
            }
        } else {
            Err(HsmError::DeviceError("Response too short".to_string()))
        }
    }

    /// Get public key from derivation path
    fn get_public_key_internal(&self, path: &str) -> HsmResult<Vec<u8>> {
        let path_bytes = self.encode_derivation_path(path)?;

        let response = self.send_apdu(
            NEO_CLA,
            NeoInstruction::GetPublicKey as u8,
            0x00, // Don't display on device
            0x00,
            &path_bytes,
        )?;

        // Response format: [pubkey_len, pubkey..., address_len, address...]
        if response.is_empty() {
            return Err(HsmError::LedgerError("Empty response".to_string()));
        }

        let pubkey_len = response[0] as usize;
        if response.len() < 1 + pubkey_len {
            return Err(HsmError::LedgerError(
                "Invalid public key response".to_string(),
            ));
        }

        let public_key = response[1..1 + pubkey_len].to_vec();
        normalize_public_key(&public_key)
    }

    /// Encode derivation path for APDU
    fn encode_derivation_path(&self, path: &str) -> HsmResult<Vec<u8>> {
        let components = super::parse_derivation_path(path)
            .ok_or_else(|| HsmError::InvalidDerivationPath(path.to_string()))?;

        let mut data = vec![5u8]; // 5 path components

        // Encode each component (hardened paths have 0x80000000 added)
        let encode = |val: u32, hardened: bool| -> [u8; 4] {
            let v = if hardened { val | 0x80000000 } else { val };
            v.to_be_bytes()
        };

        data.extend_from_slice(&encode(components.0, true)); // purpose (44')
        data.extend_from_slice(&encode(components.1, true)); // coin_type (888')
        data.extend_from_slice(&encode(components.2, true)); // account'
        data.extend_from_slice(&encode(components.3, false)); // change
        data.extend_from_slice(&encode(components.4, false)); // index

        Ok(data)
    }
}

#[async_trait]
impl HsmSigner for LedgerSigner {
    fn device_info(&self) -> &HsmDeviceInfo {
        &self.device_info
    }

    fn is_ready(&self) -> bool {
        *self.is_ready.read()
    }

    async fn unlock(&self, _pin: &str) -> HsmResult<()> {
        // Ledger handles PIN on-device
        // We just verify the Neo app is open by getting version
        let _ = self.send_apdu(NEO_CLA, NeoInstruction::GetVersion as u8, 0, 0, &[])?;

        *self.is_locked.write() = false;
        *self.is_ready.write() = true;

        tracing::info!(
            target: "neo::hsm",
            "Ledger device ready: {} {}",
            self.device_info.manufacturer,
            self.device_info.model
        );

        Ok(())
    }

    fn lock(&self) {
        *self.is_locked.write() = true;
        *self.is_ready.write() = false;
    }

    fn is_locked(&self) -> bool {
        *self.is_locked.read()
    }

    async fn list_keys(&self) -> HsmResult<Vec<HsmKeyInfo>> {
        // Return first 5 keys from default derivation paths
        let mut keys = Vec::new();
        for i in 0..5 {
            let path = super::neo_derivation_path(0, i);
            match self.get_key(&path).await {
                Ok(key) => keys.push(key),
                Err(_) => break, // Stop on first error
            }
        }
        Ok(keys)
    }

    async fn get_key(&self, key_id: &str) -> HsmResult<HsmKeyInfo> {
        let public_key = self.get_public_key_internal(key_id)?;
        let script_hash = script_hash_from_public_key(&public_key)?;

        Ok(HsmKeyInfo::new(key_id, public_key, script_hash).with_derivation_path(key_id))
    }

    async fn sign(&self, key_id: &str, data: &[u8]) -> HsmResult<Vec<u8>> {
        if self.is_locked() {
            return Err(HsmError::PinRequired);
        }

        let path_bytes = self.encode_derivation_path(key_id)?;

        // Build sign request: path + data
        let mut payload = path_bytes;
        payload.extend_from_slice(data);

        let signature = self.send_apdu(
            NEO_CLA,
            NeoInstruction::SignTransaction as u8,
            0x00,
            0x00,
            &payload,
        )?;

        // Ledger returns DER-encoded signature, convert to raw r||s
        let raw_sig = self.der_to_raw(&signature)?;

        Ok(raw_sig)
    }

    async fn get_public_key(&self, key_id: &str) -> HsmResult<Vec<u8>> {
        self.get_public_key_internal(key_id)
    }

    async fn verify_device(&self) -> HsmResult<bool> {
        // Try to get version to verify device is genuine
        match self.send_apdu(NEO_CLA, NeoInstruction::GetVersion as u8, 0, 0, &[]) {
            Ok(_) => Ok(true),
            Err(_) => Ok(false),
        }
    }
}

impl LedgerSigner {
    /// Convert DER-encoded signature to raw r||s format
    fn der_to_raw(&self, der: &[u8]) -> HsmResult<Vec<u8>> {
        // DER format: 0x30 len 0x02 r_len r... 0x02 s_len s...
        if der.len() < 8 || der[0] != 0x30 {
            return Err(HsmError::SigningFailed("Invalid DER signature".to_string()));
        }

        let mut pos = 2; // Skip 0x30 and length

        // Parse r
        if der[pos] != 0x02 {
            return Err(HsmError::SigningFailed("Invalid r marker".to_string()));
        }
        pos += 1;
        let r_len = der[pos] as usize;
        pos += 1;
        let r_start = if der[pos] == 0x00 { pos + 1 } else { pos };
        let r = &der[r_start..pos + r_len];
        pos += r_len;

        // Parse s
        if der[pos] != 0x02 {
            return Err(HsmError::SigningFailed("Invalid s marker".to_string()));
        }
        pos += 1;
        let s_len = der[pos] as usize;
        pos += 1;
        let s_start = if der[pos] == 0x00 { pos + 1 } else { pos };
        let s = &der[s_start..pos + s_len];

        // Pad to 32 bytes each
        let mut raw = vec![0u8; 64];
        let r_offset = 32 - r.len().min(32);
        let s_offset = 32 - s.len().min(32);
        raw[r_offset..32].copy_from_slice(&r[r.len().saturating_sub(32)..]);
        raw[32 + s_offset..64].copy_from_slice(&s[s.len().saturating_sub(32)..]);

        Ok(raw)
    }
}
