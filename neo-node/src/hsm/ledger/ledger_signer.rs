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
            // hidapi's manufacturer_string()/product_string() return Option<&str>;
            // unwrap_or takes the inner &str directly (the previous double
            // unwrap_or(Some(..)) did not compile - this module is
            // feature-gated behind `ledger` and had never been compiled).
            manufacturer: device.manufacturer_string().unwrap_or("Ledger").to_string(),
            model: device.product_string().unwrap_or("Unknown").to_string(),
            serial_number: device.serial_number().map(|s| s.to_string()),
            firmware_version: None,
            is_connected: true,
            requires_pin: true,
        })
    }

    /// Send an APDU command to the Ledger.
    ///
    /// Implements the Ledger HID chunked transport: requests longer than one
    /// report are split across continuation frames with increasing sequence
    /// numbers, and responses are reassembled the same way. (R17: the
    /// previous single-report write truncated real signing payloads — the
    /// 36-byte network+hash input plus the 21-byte path exceeds the 57-byte
    /// first-frame capacity — and only the first response report was read,
    /// although DER signatures usually span several.)
    fn send_apdu(&self, cla: u8, ins: u8, p1: u8, p2: u8, data: &[u8]) -> HsmResult<Vec<u8>> {
        /// HID report channel used by the Ledger transport.
        const CHANNEL: [u8; 2] = [0x01, 0x01];
        /// Framing tag for U2F-style HID messages.
        const TAG: u8 = 0x05;
        /// Payload capacity of the first frame (7-byte header + payload).
        const FIRST_FRAME_PAYLOAD: usize = 57;
        /// Payload capacity of continuation frames (5-byte header).
        const CONT_FRAME_PAYLOAD: usize = 64;

        if data.len() > u8::MAX as usize {
            return Err(HsmError::DeviceError(format!(
                "APDU data too long: {} bytes (max 255)",
                data.len()
            )));
        }

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

        // Send with chunking
        let mut offset = 0usize;
        let mut sequence: u16 = 0;
        while offset < apdu.len() || sequence == 0 {
            let mut report = vec![0x00]; // Report ID
            report.extend_from_slice(&CHANNEL);
            report.push(TAG);
            report.extend_from_slice(&sequence.to_be_bytes());
            if sequence == 0 {
                report.extend_from_slice(&(apdu.len() as u16).to_be_bytes());
                let take = apdu.len().min(FIRST_FRAME_PAYLOAD);
                report.extend_from_slice(&apdu[..take]);
                offset += take;
            } else {
                let take = (apdu.len() - offset).min(CONT_FRAME_PAYLOAD);
                report.extend_from_slice(&apdu[offset..offset + take]);
                offset += take;
            }
            report.resize(65, 0x00);

            device
                .write(&report)
                .map_err(|e| HsmError::DeviceError(format!("Write failed: {}", e)))?;
            sequence = sequence.wrapping_add(1);
        }

        // Read and reassemble the response
        let mut payload: Vec<u8> = Vec::new();
        let mut expected_len: Option<usize> = None;
        let mut expected_sequence: u16 = 0;
        let mut buf = [0u8; 65];
        loop {
            let read_len = device
                .read_timeout(&mut buf, 30000)
                .map_err(|e| HsmError::DeviceError(format!("Read failed: {}", e)))?;

            // Platform-dependent layouts: some hidapi backends deliver the
            // report-id slot, others start directly with the channel bytes.
            let (header_start, read_len) = if buf[0] == CHANNEL[0] && buf[1] == CHANNEL[1] {
                (0usize, read_len)
            } else if buf[1] == CHANNEL[0] && buf[2] == CHANNEL[1] {
                (1usize, read_len)
            } else {
                return Err(HsmError::DeviceError(
                    "Invalid response framing".to_string(),
                ));
            };
            let frame = buf
                .get(header_start..read_len.min(buf.len()))
                .ok_or_else(|| HsmError::DeviceError("Invalid response framing".to_string()))?;
            if frame.len() < 5 || frame[2] != TAG {
                return Err(HsmError::DeviceError(
                    "Invalid response framing".to_string(),
                ));
            }
            let seq = ((frame[3] as u16) << 8) | frame[4] as u16;
            if seq != expected_sequence {
                return Err(HsmError::DeviceError(format!(
                    "Unexpected response sequence {} (expected {})",
                    seq, expected_sequence
                )));
            }

            let header_end = if expected_sequence == 0 {
                if frame.len() < 7 {
                    return Err(HsmError::DeviceError("Invalid response length".to_string()));
                }
                let total = ((frame[5] as usize) << 8) | frame[6] as usize;
                expected_len = Some(total);
                7
            } else {
                5
            };

            let mut chunk = &frame[header_end.min(frame.len())..];
            if let Some(total) = expected_len {
                let remaining = total.saturating_sub(payload.len());
                if chunk.len() > remaining {
                    chunk = &chunk[..remaining];
                }
            }
            payload.extend_from_slice(chunk);
            expected_sequence = expected_sequence.wrapping_add(1);

            if expected_len.map_or(true, |total| payload.len() >= total) {
                break;
            }
        }
        if let Some(total) = expected_len {
            payload.truncate(total);
        }

        // Check status word (last 2 bytes)
        if payload.len() < 2 {
            return Err(HsmError::DeviceError("Response too short".to_string()));
        }
        let sw = ((payload[payload.len() - 2] as u16) << 8) | payload[payload.len() - 1] as u16;
        match sw {
            0x9000 => Ok(payload[..payload.len() - 2].to_vec()),
            0x6985 => Err(HsmError::UserRejected),
            0x6982 => Err(HsmError::PinLocked),
            0x6700 => Err(HsmError::LedgerError("Invalid data length".to_string())),
            0x6E00 => Err(HsmError::LedgerError("Neo app not open".to_string())),
            _ => Err(HsmError::LedgerError(format!("Status: 0x{:04X}", sw))),
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
    /// Convert DER-encoded signature to raw r||s format.
    ///
    /// All indices are bounds-checked: a malformed (or hostile) device
    /// response must produce an error, never a panic (R17).
    fn der_to_raw(&self, der: &[u8]) -> HsmResult<Vec<u8>> {
        const TRUNCATED: &str = "Truncated DER signature";

        if der.len() < 8 {
            return Err(HsmError::SigningFailed(TRUNCATED.to_string()));
        }
        if der[0] != 0x30 {
            return Err(HsmError::SigningFailed("Invalid DER signature".to_string()));
        }

        let byte_at = |index: usize| -> HsmResult<u8> {
            der.get(index)
                .copied()
                .ok_or_else(|| HsmError::SigningFailed(TRUNCATED.to_string()))
        };

        // Parse r
        let mut pos = 2; // Skip 0x30 and length
        if byte_at(pos)? != 0x02 {
            return Err(HsmError::SigningFailed("Invalid r marker".to_string()));
        }
        pos += 1;
        let r_len = byte_at(pos)? as usize;
        pos += 1;
        let r_start = if byte_at(pos)? == 0x00 { pos + 1 } else { pos };
        let r_end = pos + r_len;
        if r_end > der.len() || r_start > r_end {
            return Err(HsmError::SigningFailed(TRUNCATED.to_string()));
        }
        let r = &der[r_start..r_end];
        pos = r_end;

        // Parse s
        if byte_at(pos)? != 0x02 {
            return Err(HsmError::SigningFailed("Invalid s marker".to_string()));
        }
        pos += 1;
        let s_len = byte_at(pos)? as usize;
        pos += 1;
        let s_start = if byte_at(pos)? == 0x00 { pos + 1 } else { pos };
        let s_end = pos + s_len;
        if s_end > der.len() || s_start > s_end {
            return Err(HsmError::SigningFailed(TRUNCATED.to_string()));
        }
        let s = &der[s_start..s_end];

        if r.is_empty() || s.is_empty() {
            return Err(HsmError::SigningFailed("Empty DER component".to_string()));
        }

        // Right-align each big-endian component into 32 bytes, truncating
        // excess high bytes.
        let mut raw = vec![0u8; 64];
        let r_src = r.len().saturating_sub(32);
        raw[32 - (r.len() - r_src)..32].copy_from_slice(&r[r_src..]);
        let s_src = s.len().saturating_sub(32);
        raw[64 - (s.len() - s_src)..64].copy_from_slice(&s[s_src..]);

        Ok(raw)
    }
}
