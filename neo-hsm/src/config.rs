//! HSM Configuration

use crate::device::HsmDeviceType;
use std::path::PathBuf;

/// HSM configuration
#[derive(Debug, Clone)]
pub struct HsmConfig {
    /// Type of HSM device
    pub device_type: HsmDeviceType,

    /// Device slot or index (0-based)
    pub slot: u64,

    /// Key ID or derivation path
    pub key_id: Option<String>,

    /// PKCS#11 library path (required for PKCS#11 devices)
    pub pkcs11_lib: Option<PathBuf>,

    /// Whether to skip PIN prompt (for testing only)
    pub skip_pin: bool,

    /// Maximum PIN retry attempts
    pub max_pin_attempts: u32,

    /// Connection timeout in milliseconds
    pub timeout_ms: u64,
}

impl Default for HsmConfig {
    fn default() -> Self {
        Self {
            device_type: HsmDeviceType::Simulation,
            slot: 0,
            key_id: None,
            pkcs11_lib: None,
            skip_pin: false,
            max_pin_attempts: 3,
            timeout_ms: 30_000,
        }
    }
}

impl HsmConfig {
    /// Create a new HSM configuration for Ledger
    pub fn ledger(slot: u64) -> Self {
        Self {
            device_type: HsmDeviceType::Ledger,
            slot,
            ..Default::default()
        }
    }

    /// Create a new HSM configuration for PKCS#11
    pub fn pkcs11(lib_path: PathBuf, slot: u64) -> Self {
        Self {
            device_type: HsmDeviceType::Pkcs11,
            slot,
            pkcs11_lib: Some(lib_path),
            ..Default::default()
        }
    }

    /// Create a new HSM configuration for simulation
    pub fn simulation() -> Self {
        Self {
            device_type: HsmDeviceType::Simulation,
            skip_pin: true,
            ..Default::default()
        }
    }

    /// Set the key ID or derivation path
    pub fn with_key_id(mut self, key_id: impl Into<String>) -> Self {
        self.key_id = Some(key_id.into());
        self
    }

    /// Set whether to skip PIN prompt
    pub fn with_skip_pin(mut self, skip: bool) -> Self {
        self.skip_pin = skip;
        self
    }
}
