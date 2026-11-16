//! HSM Device information

use serde::{Deserialize, Serialize};

/// Type of HSM device
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum HsmDeviceType {
    /// Ledger hardware wallet (Nano S/X/S Plus)
    Ledger,
    /// Generic PKCS#11 HSM
    Pkcs11,
}

impl std::fmt::Display for HsmDeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HsmDeviceType::Ledger => write!(f, "Ledger"),
            HsmDeviceType::Pkcs11 => write!(f, "PKCS#11"),
        }
    }
}

impl std::str::FromStr for HsmDeviceType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "ledger" => Ok(HsmDeviceType::Ledger),
            "pkcs11" => Ok(HsmDeviceType::Pkcs11),
            _ => Err(format!(
                "Unknown device type: {s}; expected ledger or pkcs11"
            )),
        }
    }
}

/// Information about an HSM device
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HsmDeviceInfo {
    /// Type of device
    pub device_type: HsmDeviceType,

    /// Manufacturer name
    pub manufacturer: String,

    /// Model name
    pub model: String,

    /// Serial number (if available)
    pub serial_number: Option<String>,

    /// Firmware version (if available)
    pub firmware_version: Option<String>,

    /// Whether the device is currently connected
    pub is_connected: bool,

    /// Whether the device requires PIN
    pub requires_pin: bool,
}
