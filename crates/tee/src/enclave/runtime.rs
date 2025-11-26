//! Enclave runtime implementation

use crate::error::{TeeError, TeeResult};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Enclave configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnclaveConfig {
    /// Path to store sealed data
    pub sealed_data_path: PathBuf,
    /// Enable debug mode (allows inspection of enclave memory)
    pub debug_mode: bool,
    /// Maximum heap size for the enclave (in MB)
    pub heap_size_mb: usize,
    /// Number of TCS (Thread Control Structures)
    pub tcs_count: usize,
    /// Enable simulation mode
    pub simulation: bool,
}

impl Default for EnclaveConfig {
    fn default() -> Self {
        Self {
            sealed_data_path: PathBuf::from("./tee_data"),
            debug_mode: false,
            heap_size_mb: 256,
            tcs_count: 4,
            #[cfg(feature = "simulation")]
            simulation: true,
            #[cfg(not(feature = "simulation"))]
            simulation: false,
        }
    }
}

/// Current state of the enclave
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnclaveState {
    /// Enclave not initialized
    Uninitialized,
    /// Enclave is initializing
    Initializing,
    /// Enclave is ready for operations
    Ready,
    /// Enclave encountered an error
    Error,
    /// Enclave is shutting down
    ShuttingDown,
}

/// TEE Enclave runtime
pub struct TeeEnclave {
    config: EnclaveConfig,
    state: RwLock<EnclaveState>,
    /// Enclave-specific sealing key (derived from hardware or simulated)
    sealing_key: RwLock<Option<[u8; 32]>>,
    /// Monotonic counter for replay protection
    monotonic_counter: RwLock<u64>,
}

impl TeeEnclave {
    /// Create a new TEE enclave with the given configuration
    pub fn new(config: EnclaveConfig) -> Self {
        Self {
            config,
            state: RwLock::new(EnclaveState::Uninitialized),
            sealing_key: RwLock::new(None),
            monotonic_counter: RwLock::new(0),
        }
    }

    /// Initialize the enclave
    pub fn initialize(&self) -> TeeResult<()> {
        let mut state = self.state.write();
        if *state != EnclaveState::Uninitialized {
            return Err(TeeError::EnclaveInitFailed(
                "Enclave already initialized".to_string(),
            ));
        }

        *state = EnclaveState::Initializing;
        info!("Initializing TEE enclave (simulation={})", self.config.simulation);

        // Create sealed data directory if it doesn't exist
        std::fs::create_dir_all(&self.config.sealed_data_path)?;

        // Initialize sealing key
        let sealing_key = self.derive_sealing_key()?;
        *self.sealing_key.write() = Some(sealing_key);

        // Load monotonic counter from sealed storage
        self.load_monotonic_counter()?;

        *state = EnclaveState::Ready;
        info!("TEE enclave initialized successfully");
        Ok(())
    }

    /// Check if the enclave is ready
    pub fn is_ready(&self) -> bool {
        *self.state.read() == EnclaveState::Ready
    }

    /// Get current enclave state
    pub fn state(&self) -> EnclaveState {
        *self.state.read()
    }

    /// Get the enclave configuration
    pub fn config(&self) -> &EnclaveConfig {
        &self.config
    }

    /// Get the sealing key (only available inside enclave)
    pub(crate) fn sealing_key(&self) -> TeeResult<[u8; 32]> {
        self.sealing_key
            .read()
            .ok_or(TeeError::EnclaveNotInitialized)
    }

    /// Increment and return the monotonic counter
    pub fn increment_counter(&self) -> TeeResult<u64> {
        if !self.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }

        let mut counter = self.monotonic_counter.write();
        *counter += 1;
        let value = *counter;

        // Persist counter to sealed storage
        self.save_monotonic_counter(value)?;

        Ok(value)
    }

    /// Get current monotonic counter value
    pub fn current_counter(&self) -> TeeResult<u64> {
        if !self.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }
        Ok(*self.monotonic_counter.read())
    }

    /// Shutdown the enclave
    pub fn shutdown(&self) -> TeeResult<()> {
        let mut state = self.state.write();
        if *state != EnclaveState::Ready {
            return Ok(());
        }

        *state = EnclaveState::ShuttingDown;
        info!("Shutting down TEE enclave");

        // Clear sensitive data
        if let Some(key) = self.sealing_key.write().as_mut() {
            key.fill(0);
        }
        *self.sealing_key.write() = None;

        *state = EnclaveState::Uninitialized;
        Ok(())
    }

    /// Derive sealing key from hardware or simulation
    fn derive_sealing_key(&self) -> TeeResult<[u8; 32]> {
        #[cfg(feature = "sgx-hw")]
        {
            self.derive_sgx_sealing_key()
        }

        #[cfg(not(feature = "sgx-hw"))]
        {
            self.derive_simulated_sealing_key()
        }
    }

    /// Derive sealing key from SGX hardware
    #[cfg(feature = "sgx-hw")]
    fn derive_sgx_sealing_key(&self) -> TeeResult<[u8; 32]> {
        use sgx_isa::{Keyname, Keypolicy, Keyrequest};

        // Create key request for sealing
        let keyrequest = Keyrequest {
            keyname: Keyname::Seal as u16,
            keypolicy: Keypolicy::MRSIGNER,
            ..Default::default()
        };

        // Get the key from SGX hardware
        let key = keyrequest
            .egetkey()
            .map_err(|e| TeeError::CryptoError(format!("SGX EGETKEY failed: {:?}", e)))?;

        // Expand to 32 bytes using SHA-256
        use sha2::{Sha256, Digest};
        let mut hasher = Sha256::new();
        hasher.update(&key);
        let result = hasher.finalize();

        let mut sealing_key = [0u8; 32];
        sealing_key.copy_from_slice(&result);
        Ok(sealing_key)
    }

    /// Derive simulated sealing key for testing
    #[cfg(not(feature = "sgx-hw"))]
    fn derive_simulated_sealing_key(&self) -> TeeResult<[u8; 32]> {
        use sha2::{Sha256, Digest};

        // In simulation mode, derive key from a machine-specific identifier
        // In production, this would come from SGX EGETKEY
        let machine_id = self.get_machine_identifier();

        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-simulation-key-v1");
        hasher.update(&machine_id);
        hasher.update(self.config.sealed_data_path.to_string_lossy().as_bytes());
        let result = hasher.finalize();

        let mut sealing_key = [0u8; 32];
        sealing_key.copy_from_slice(&result);

        debug!("Derived simulated sealing key");
        Ok(sealing_key)
    }

    /// Get a machine-specific identifier for simulation mode
    #[cfg(not(feature = "sgx-hw"))]
    fn get_machine_identifier(&self) -> Vec<u8> {
        use sha2::{Sha256, Digest};

        let mut hasher = Sha256::new();

        // Combine various system identifiers
        if let Ok(hostname) = std::env::var("HOSTNAME") {
            hasher.update(hostname.as_bytes());
        }
        if let Ok(user) = std::env::var("USER") {
            hasher.update(user.as_bytes());
        }

        // Add some randomness on first run, persist it
        let id_file = self.config.sealed_data_path.join(".machine_id");
        let machine_id = if id_file.exists() {
            std::fs::read(&id_file).unwrap_or_else(|_| vec![0u8; 32])
        } else {
            let id: [u8; 32] = rand::random();
            let _ = std::fs::write(&id_file, &id);
            id.to_vec()
        };

        hasher.update(&machine_id);
        hasher.finalize().to_vec()
    }

    fn counter_file_path(&self) -> PathBuf {
        self.config.sealed_data_path.join(".monotonic_counter")
    }

    fn load_monotonic_counter(&self) -> TeeResult<()> {
        let path = self.counter_file_path();
        if path.exists() {
            let data = std::fs::read(&path)?;
            if data.len() >= 8 {
                let value = u64::from_le_bytes(data[..8].try_into().unwrap());
                *self.monotonic_counter.write() = value;
                debug!("Loaded monotonic counter: {}", value);
            }
        }
        Ok(())
    }

    fn save_monotonic_counter(&self, value: u64) -> TeeResult<()> {
        let path = self.counter_file_path();
        std::fs::write(&path, &value.to_le_bytes())?;
        Ok(())
    }
}

impl Drop for TeeEnclave {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_enclave_lifecycle() {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = TeeEnclave::new(config);
        assert_eq!(enclave.state(), EnclaveState::Uninitialized);

        enclave.initialize().unwrap();
        assert_eq!(enclave.state(), EnclaveState::Ready);
        assert!(enclave.is_ready());

        // Test monotonic counter
        let c1 = enclave.increment_counter().unwrap();
        let c2 = enclave.increment_counter().unwrap();
        assert_eq!(c2, c1 + 1);

        enclave.shutdown().unwrap();
        assert_eq!(enclave.state(), EnclaveState::Uninitialized);
    }
}
