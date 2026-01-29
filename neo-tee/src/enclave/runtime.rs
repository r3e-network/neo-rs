//! Enclave runtime implementation

use crate::error::{EnclaveInitError, TeeError, TeeResult};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tracing::{debug, error, info, warn};

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
    /// Expected MRENCLAVE for verification (optional, for production)
    pub expected_mrenclave: Option<[u8; 32]>,
    /// Expected MRSIGNER for verification (optional)
    pub expected_mrsigner: Option<[u8; 32]>,
    /// Allow debug mode in production (default: false)
    pub allow_debug_in_production: bool,
    /// Minimum ISV SVN (security version) allowed
    pub min_isv_svn: u16,
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
            expected_mrenclave: None,
            expected_mrsigner: None,
            allow_debug_in_production: false,
            min_isv_svn: 1,
        }
    }
}

impl EnclaveConfig {
    /// Validate the configuration
    pub fn validate(&self) -> TeeResult<()> {
        // Validate heap size
        if self.heap_size_mb == 0 || self.heap_size_mb > 4096 {
            return Err(TeeError::enclave_init_error(
                EnclaveInitError::InvalidConfiguration,
                format!(
                    "Invalid heap_size_mb: {}, must be between 1 and 4096",
                    self.heap_size_mb
                ),
            ));
        }

        // Validate TCS count
        if self.tcs_count == 0 || self.tcs_count > 256 {
            return Err(TeeError::enclave_init_error(
                EnclaveInitError::InvalidConfiguration,
                format!(
                    "Invalid tcs_count: {}, must be between 1 and 256",
                    self.tcs_count
                ),
            ));
        }

        // Validate sealed data path
        if self.sealed_data_path.as_os_str().is_empty() {
            return Err(TeeError::enclave_init_error(
                EnclaveInitError::InvalidConfiguration,
                "sealed_data_path cannot be empty",
            ));
        }

        // Check debug mode restrictions
        if self.debug_mode && !self.simulation && !self.allow_debug_in_production {
            return Err(TeeError::enclave_init_error(
                EnclaveInitError::DebugNotAllowed,
                "Debug mode not allowed in production (set allow_debug_in_production=true to override)",
            ));
        }

        Ok(())
    }

    /// Check if MRENCLAVE verification is required
    pub fn requires_mrenclave_verification(&self) -> bool {
        self.expected_mrenclave.is_some() && !self.simulation
    }

    /// Check if MRSIGNER verification is required
    pub fn requires_mrsigner_verification(&self) -> bool {
        self.expected_mrsigner.is_some() && !self.simulation
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

/// Detailed initialization result
#[derive(Debug, Clone)]
pub struct InitResult {
    pub state: EnclaveState,
    pub sealing_key_derived: bool,
    pub counter_loaded: bool,
    pub hardware_attestation_available: bool,
}

/// TEE Enclave runtime
pub struct TeeEnclave {
    config: EnclaveConfig,
    state: RwLock<EnclaveState>,
    /// Enclave-specific sealing key (derived from hardware or simulated)
    sealing_key: RwLock<Option<[u8; 32]>>,
    /// Monotonic counter for replay protection
    monotonic_counter: RwLock<u64>,
    /// Initialization error details (if any)
    init_error: RwLock<Option<String>>,
}

impl TeeEnclave {
    /// Create a new TEE enclave with the given configuration
    pub fn new(config: EnclaveConfig) -> Self {
        Self {
            config,
            state: RwLock::new(EnclaveState::Uninitialized),
            sealing_key: RwLock::new(None),
            monotonic_counter: RwLock::new(0),
            init_error: RwLock::new(None),
        }
    }

    /// Initialize the enclave with comprehensive error handling
    pub fn initialize(&self) -> TeeResult<InitResult> {
        let mut state = self.state.write();

        // Check if already initialized
        if *state != EnclaveState::Uninitialized {
            return Err(TeeError::EnclaveInitError {
                error: EnclaveInitError::AlreadyInitialized,
                context: format!("Current state: {:?}", *state),
            });
        }

        *state = EnclaveState::Initializing;
        drop(state); // Release lock during initialization

        info!(
            "Initializing TEE enclave (simulation={})",
            self.config.simulation
        );

        // Step 1: Validate configuration
        if let Err(e) = self.config.validate() {
            self.set_error_state(format!("Configuration validation failed: {}", e));
            return Err(e);
        }

        // Step 2: Create sealed data directory
        if let Err(e) = self.create_sealed_data_directory() {
            self.set_error_state(format!("Directory creation failed: {}", e));
            return Err(e);
        }

        // Step 3: Initialize sealing key
        let sealing_key = match self.derive_sealing_key() {
            Ok(key) => {
                debug!("Successfully derived sealing key");
                key
            }
            Err(e) => {
                self.set_error_state(format!("Sealing key derivation failed: {}", e));
                return Err(TeeError::enclave_init_error(
                    EnclaveInitError::SealingKeyDerivationFailed,
                    e.to_string(),
                ));
            }
        };
        *self.sealing_key.write() = Some(sealing_key);

        // Step 4: Load monotonic counter
        let counter_loaded = match self.load_monotonic_counter() {
            Ok(()) => {
                debug!("Successfully loaded monotonic counter");
                true
            }
            Err(e) => {
                warn!("Failed to load monotonic counter (starting from 0): {}", e);
                false
            }
        };

        // Step 5: Check hardware attestation availability
        let hw_attestation_available = self.check_hardware_attestation();

        // Finalize state
        *self.state.write() = EnclaveState::Ready;
        info!("TEE enclave initialized successfully");

        Ok(InitResult {
            state: EnclaveState::Ready,
            sealing_key_derived: true,
            counter_loaded,
            hardware_attestation_available: hw_attestation_available,
        })
    }

    /// Create the sealed data directory with secure permissions
    fn create_sealed_data_directory(&self) -> TeeResult<()> {
        let path = &self.config.sealed_data_path;

        if path.exists() {
            // Verify it's a directory and we have write permissions
            if !path.is_dir() {
                return Err(TeeError::enclave_init_error(
                    EnclaveInitError::DirectoryCreationFailed,
                    format!("Path exists but is not a directory: {:?}", path),
                ));
            }

            // Test write permissions
            let test_file = path.join(".write_test");
            match std::fs::File::create(&test_file) {
                Ok(_) => {
                    let _ = std::fs::remove_file(&test_file);
                }
                Err(e) => {
                    return Err(TeeError::enclave_init_error(
                        EnclaveInitError::DirectoryCreationFailed,
                        format!("Directory not writable: {:?} - {}", path, e),
                    ));
                }
            }

            return Ok(());
        }

        // Create directory with restricted permissions
        match std::fs::create_dir_all(path) {
            Ok(()) => {
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let permissions = std::fs::Permissions::from_mode(0o700);
                    if let Err(e) = std::fs::set_permissions(path, permissions) {
                        warn!("Failed to set directory permissions: {}", e);
                    }
                }
                debug!("Created sealed data directory: {:?}", path);
                Ok(())
            }
            Err(e) => Err(TeeError::enclave_init_error(
                EnclaveInitError::DirectoryCreationFailed,
                format!("Failed to create {:?}: {}", path, e),
            )),
        }
    }

    /// Set error state with message
    fn set_error_state(&self, error_msg: String) {
        error!("Enclave initialization error: {}", error_msg);
        *self.state.write() = EnclaveState::Error;
        *self.init_error.write() = Some(error_msg);
    }

    /// Check if hardware attestation is available
    fn check_hardware_attestation(&self) -> bool {
        #[cfg(feature = "sgx-hw")]
        {
            // Check if we're actually running in SGX
            match std::fs::exists("/dev/sgx_enclave") {
                Ok(true) => {
                    debug!("SGX device found, hardware attestation available");
                    true
                }
                _ => {
                    debug!("SGX device not found, hardware attestation unavailable");
                    false
                }
            }
        }
        #[cfg(not(feature = "sgx-hw"))]
        {
            false
        }
    }

    /// Get the last initialization error if any
    pub fn last_init_error(&self) -> Option<String> {
        self.init_error.read().clone()
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
        info!("TEE enclave shutdown complete");
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
        // SGX EGETKEY instruction requires running inside an SGX enclave.
        // When running outside an enclave (e.g. CI/dev), fall back to a deterministic simulated key.
        // On real SGX hardware this should use the EGETKEY instruction via the sgx_isa crate.
        //
        // Note: The sgx_isa crate's Keyrequest::egetkey requires actual SGX hardware.
        // When not running inside an enclave, this function uses simulation.
        use sha2::{Digest, Sha256};

        if std::fs::exists("/dev/sgx_enclave").unwrap_or(false) {
            // SGX device exists but we might not be inside an enclave
            debug!("SGX hardware available but not running in enclave, using simulated key");
        } else {
            debug!("SGX hardware not available, using simulated key");
        }

        // Use HKDF-like derivation for better key separation
        let machine_id = self.get_machine_identifier();

        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-sgx-simulation-key-v2");
        hasher.update(&machine_id);
        hasher.update(self.config.sealed_data_path.to_string_lossy().as_bytes());

        // Add configuration parameters to key derivation
        hasher.update(&(self.config.heap_size_mb as u64).to_le_bytes());
        hasher.update(&(self.config.tcs_count as u64).to_le_bytes());

        let result = hasher.finalize();

        let mut sealing_key = [0u8; 32];
        sealing_key.copy_from_slice(&result);
        Ok(sealing_key)
    }

    /// Derive simulated sealing key for testing
    #[cfg(not(feature = "sgx-hw"))]
    fn derive_simulated_sealing_key(&self) -> TeeResult<[u8; 32]> {
        use sha2::{Digest, Sha256};

        // In simulation mode, derive the sealing key from a machine-specific identifier.
        // On SGX hardware, this value is derived via EGETKEY.
        let machine_id = self.get_machine_identifier();

        // Use HKDF-like two-step derivation for better security
        // Step 1: Extract PRK from machine_id
        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-simulation-extract");
        hasher.update(&machine_id);
        let prk = hasher.finalize();

        // Step 2: Expand to get final key
        let mut hasher = Sha256::new();
        hasher.update(&prk);
        hasher.update(b"neo-tee-simulation-key-v2");
        hasher.update(self.config.sealed_data_path.to_string_lossy().as_bytes());
        hasher.update(&(self.config.heap_size_mb as u64).to_le_bytes());
        hasher.update(&(self.config.tcs_count as u64).to_le_bytes());
        let result = hasher.finalize();

        let mut sealing_key = [0u8; 32];
        sealing_key.copy_from_slice(&result);

        debug!("Derived simulated sealing key using HKDF-like derivation");
        Ok(sealing_key)
    }

    /// Get a machine-specific identifier for simulation mode
    #[cfg(not(feature = "sgx-hw"))]
    fn get_machine_identifier(&self) -> Vec<u8> {
        use sha2::{Digest, Sha256};

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
            match std::fs::read(&id_file) {
                Ok(id) => {
                    if id.len() == 32 {
                        id
                    } else {
                        warn!("Invalid machine_id file size, generating new ID");
                        Self::generate_and_save_machine_id(&id_file)
                    }
                }
                Err(e) => {
                    warn!("Failed to read machine_id: {}, generating new", e);
                    Self::generate_and_save_machine_id(&id_file)
                }
            }
        } else {
            Self::generate_and_save_machine_id(&id_file)
        };

        hasher.update(&machine_id);
        hasher.finalize().to_vec()
    }

    /// Generate and save a new machine identifier
    #[cfg(not(feature = "sgx-hw"))]
    fn generate_and_save_machine_id(id_file: &PathBuf) -> Vec<u8> {
        let id: [u8; 32] = rand::random();

        // Ensure parent directory exists
        if let Some(parent) = id_file.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // Write with restrictive permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let result = std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(id_file)
                .and_then(|mut f| std::io::Write::write_all(&mut f, &id));

            if let Err(e) = result {
                warn!("Failed to persist machine_id: {}", e);
            }
        }
        #[cfg(not(unix))]
        {
            if let Err(e) = std::fs::write(id_file, id) {
                warn!("Failed to persist machine_id: {}", e);
            }
        }

        id.to_vec()
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
            } else {
                return Err(TeeError::enclave_init_error(
                    EnclaveInitError::CounterLoadFailed,
                    "Counter file too short",
                ));
            }
        }
        Ok(())
    }

    fn save_monotonic_counter(&self, value: u64) -> TeeResult<()> {
        let path = self.counter_file_path();
        // Write with restrictive permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&path)
                .and_then(|mut f| std::io::Write::write_all(&mut f, &value.to_le_bytes()))?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&path, value.to_le_bytes())?;
        }
        Ok(())
    }

    /// Verify MRENCLAVE against expected value
    pub fn verify_mrenclave(&self, actual_mrenclave: &[u8; 32]) -> TeeResult<()> {
        if let Some(expected) = self.config.expected_mrenclave {
            if &expected != actual_mrenclave {
                return Err(TeeError::mrenclave_mismatch(&expected, actual_mrenclave));
            }
            debug!("MRENCLAVE verification passed");
        }
        Ok(())
    }

    /// Verify MRSIGNER against expected value
    pub fn verify_mrsigner(&self, actual_mrsigner: &[u8; 32]) -> TeeResult<()> {
        if let Some(expected) = self.config.expected_mrsigner {
            if &expected != actual_mrsigner {
                return Err(TeeError::mrsigner_mismatch(&expected, actual_mrsigner));
            }
            debug!("MRSIGNER verification passed");
        }
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

        let result = enclave.initialize().unwrap();
        assert_eq!(result.state, EnclaveState::Ready);
        assert!(result.sealing_key_derived);
        assert_eq!(enclave.state(), EnclaveState::Ready);
        assert!(enclave.is_ready());

        // Test monotonic counter
        let c1 = enclave.increment_counter().unwrap();
        let c2 = enclave.increment_counter().unwrap();
        assert_eq!(c2, c1 + 1);

        enclave.shutdown().unwrap();
        assert_eq!(enclave.state(), EnclaveState::Uninitialized);
    }

    #[test]
    fn test_double_initialization() {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = TeeEnclave::new(config);
        enclave.initialize().unwrap();

        // Second initialization should fail
        let result = enclave.initialize();
        assert!(result.is_err());

        match result.unwrap_err() {
            TeeError::EnclaveInitError { error, .. } => {
                assert_eq!(error, EnclaveInitError::AlreadyInitialized);
            }
            _ => panic!("Expected AlreadyInitialized error"),
        }
    }

    #[test]
    fn test_invalid_configuration() {
        let temp = tempdir().unwrap();

        // Test invalid heap size
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            heap_size_mb: 0,
            simulation: true,
            ..Default::default()
        };
        let enclave = TeeEnclave::new(config);
        let result = enclave.initialize();
        assert!(result.is_err());

        // Test invalid TCS count
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            tcs_count: 0,
            simulation: true,
            ..Default::default()
        };
        let enclave = TeeEnclave::new(config);
        let result = enclave.initialize();
        assert!(result.is_err());
    }

    #[test]
    fn test_debug_mode_restriction() {
        let temp = tempdir().unwrap();

        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            debug_mode: true,
            simulation: false, // Production mode
            allow_debug_in_production: false,
            ..Default::default()
        };
        let enclave = TeeEnclave::new(config);
        let result = enclave.initialize();
        assert!(result.is_err());

        // Allow debug mode
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            debug_mode: true,
            simulation: false,
            allow_debug_in_production: true,
            ..Default::default()
        };
        let enclave = TeeEnclave::new(config);
        // Will still fail in simulation mode without SGX device, but not due to debug restriction
        // Just check the error is NOT about debug mode
        let result = enclave.initialize();
        if let Err(e) = result {
            let msg = e.to_string();
            assert!(!msg.contains("Debug mode not allowed"));
        }
    }

    #[test]
    fn test_mrenclave_verification() {
        let temp = tempdir().unwrap();
        let expected = [0x42u8; 32];

        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true, // Simulation mode skips verification
            expected_mrenclave: Some(expected),
            ..Default::default()
        };

        let enclave = TeeEnclave::new(config);
        enclave.initialize().unwrap();

        // Correct MRENCLAVE should pass
        assert!(enclave.verify_mrenclave(&expected).is_ok());

        // Wrong MRENCLAVE should fail
        let wrong = [0x00u8; 32];
        let result = enclave.verify_mrenclave(&wrong);
        assert!(result.is_err());
        match result.unwrap_err() {
            TeeError::MrEnclaveMismatch { .. } => {}
            _ => panic!("Expected MrEnclaveMismatch error"),
        }
    }

    #[test]
    fn test_mrsigner_verification() {
        let temp = tempdir().unwrap();
        let expected = [0x42u8; 32];

        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            expected_mrsigner: Some(expected),
            ..Default::default()
        };

        let enclave = TeeEnclave::new(config);
        enclave.initialize().unwrap();

        // Correct MRSIGNER should pass
        assert!(enclave.verify_mrsigner(&expected).is_ok());

        // Wrong MRSIGNER should fail
        let wrong = [0x00u8; 32];
        let result = enclave.verify_mrsigner(&wrong);
        assert!(result.is_err());
        match result.unwrap_err() {
            TeeError::MrSignerMismatch { .. } => {}
            _ => panic!("Expected MrSignerMismatch error"),
        }
    }

    #[test]
    fn test_operations_before_init() {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = TeeEnclave::new(config);

        // Operations should fail before initialization
        assert!(enclave.increment_counter().is_err());
        assert!(enclave.current_counter().is_err());
    }

    #[test]
    fn test_config_validation() {
        // Valid config
        let config = EnclaveConfig {
            heap_size_mb: 256,
            tcs_count: 4,
            ..Default::default()
        };
        assert!(config.validate().is_ok());

        // Invalid heap size (too large)
        let config = EnclaveConfig {
            heap_size_mb: 5000,
            ..Default::default()
        };
        assert!(config.validate().is_err());

        // Invalid TCS count
        let config = EnclaveConfig {
            tcs_count: 300,
            ..Default::default()
        };
        assert!(config.validate().is_err());
    }
}
