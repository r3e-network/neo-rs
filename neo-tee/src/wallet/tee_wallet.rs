//! TEE Wallet implementation

use crate::enclave::TeeEnclave;
use crate::error::{TeeError, TeeResult};
use crate::wallet::SealedKey;
use neo_crypto::{Crypto, Secp256r1Crypto};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{info, warn};

/// TEE-protected wallet that stores keys in sealed format
pub struct TeeWallet {
    /// Reference to the TEE enclave
    enclave: Arc<TeeEnclave>,
    /// Wallet name
    name: String,
    /// Path to wallet file
    path: PathBuf,
    /// Sealed keys indexed by script hash
    keys: RwLock<HashMap<[u8; 20], SealedKey>>,
    /// Default account script hash
    default_account: RwLock<Option<[u8; 20]>>,
    /// Whether wallet is locked
    locked: RwLock<bool>,
}

/// Wallet metadata stored on disk
#[derive(Debug, Clone, Serialize, Deserialize)]
struct WalletMetadata {
    name: String,
    version: String,
    default_account: Option<String>,
    created_at: u64,
    keys: Vec<String>, // Filenames of sealed keys
}

impl TeeWallet {
    /// Create a new TEE wallet
    pub fn create(enclave: Arc<TeeEnclave>, name: &str, path: &Path) -> TeeResult<Self> {
        if !enclave.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }

        // Create wallet directory
        std::fs::create_dir_all(path)?;

        let wallet = Self {
            enclave,
            name: name.to_string(),
            path: path.to_path_buf(),
            keys: RwLock::new(HashMap::new()),
            default_account: RwLock::new(None),
            locked: RwLock::new(false),
        };

        wallet.save_metadata()?;
        info!("Created new TEE wallet: {}", name);

        Ok(wallet)
    }

    /// Open an existing TEE wallet
    pub fn open(enclave: Arc<TeeEnclave>, path: &Path) -> TeeResult<Self> {
        if !enclave.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }

        let metadata_path = path.join("wallet.json");
        if !metadata_path.exists() {
            return Err(TeeError::Other("Wallet not found".to_string()));
        }

        let metadata_json = std::fs::read_to_string(&metadata_path)?;
        let metadata: WalletMetadata = serde_json::from_str(&metadata_json)?;

        let mut keys = HashMap::new();

        // Load all sealed keys
        for key_filename in &metadata.keys {
            let key_path = path.join(key_filename);
            if key_path.exists() {
                match SealedKey::load_from_file(&key_path) {
                    Ok(sealed_key) => {
                        keys.insert(sealed_key.script_hash, sealed_key);
                    }
                    Err(e) => {
                        warn!("Failed to load key {}: {}", key_filename, e);
                    }
                }
            }
        }

        let default_account = metadata.default_account.and_then(|addr| {
            keys.values()
                .find(|k| k.address() == addr)
                .map(|k| k.script_hash)
        });

        info!("Opened TEE wallet: {} ({} keys)", metadata.name, keys.len());

        Ok(Self {
            enclave,
            name: metadata.name,
            path: path.to_path_buf(),
            keys: RwLock::new(keys),
            default_account: RwLock::new(default_account),
            locked: RwLock::new(false),
        })
    }

    /// Get wallet name
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Get wallet path
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Check if wallet is locked
    pub fn is_locked(&self) -> bool {
        *self.locked.read()
    }

    /// Lock the wallet
    pub fn lock(&self) {
        *self.locked.write() = true;
        info!("Wallet locked: {}", self.name);
    }

    /// Unlock the wallet (in TEE, this is implicit as keys are sealed)
    pub fn unlock(&self) -> TeeResult<()> {
        if !self.enclave.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }
        *self.locked.write() = false;
        info!("Wallet unlocked: {}", self.name);
        Ok(())
    }

    /// Create a new key pair and add to wallet
    pub fn create_key(&self, label: Option<String>) -> TeeResult<SealedKey> {
        if self.is_locked() {
            return Err(TeeError::Other("Wallet is locked".to_string()));
        }

        // Generate a new secp256r1 keypair (Neo N3 primary curve).
        let private_key = Secp256r1Crypto::generate_private_key();
        let public_key = self.derive_public_key(&private_key)?;
        let script_hash = self.compute_script_hash(&public_key)?;

        let sealed_key = SealedKey::seal(
            &self.enclave,
            &private_key,
            &public_key,
            &script_hash,
            label,
        )?;

        // Save key to file
        let key_filename = format!("key_{}.json", hex::encode(&script_hash[..8]));
        let key_path = self.path.join(&key_filename);
        sealed_key.save_to_file(&key_path)?;

        // Add to memory
        self.keys.write().insert(script_hash, sealed_key.clone());

        // Set as default if first key
        if self.default_account.read().is_none() {
            *self.default_account.write() = Some(script_hash);
        }

        self.save_metadata()?;

        info!("Created new key in TEE wallet: {}", sealed_key.address());
        Ok(sealed_key)
    }

    /// Import an existing private key
    pub fn import_key(&self, private_key: &[u8], label: Option<String>) -> TeeResult<SealedKey> {
        if self.is_locked() {
            return Err(TeeError::Other("Wallet is locked".to_string()));
        }

        let key: &[u8; 32] = private_key
            .try_into()
            .map_err(|_| TeeError::InvalidKeyFormat)?;
        let public_key = self.derive_public_key(key)?;
        let script_hash = self.compute_script_hash(&public_key)?;

        // Check if key already exists
        if self.keys.read().contains_key(&script_hash) {
            return Err(TeeError::Other("Key already exists in wallet".to_string()));
        }

        let sealed_key =
            SealedKey::seal(&self.enclave, private_key, &public_key, &script_hash, label)?;

        // Save key to file
        let key_filename = format!("key_{}.json", hex::encode(&script_hash[..8]));
        let key_path = self.path.join(&key_filename);
        sealed_key.save_to_file(&key_path)?;

        // Add to memory
        self.keys.write().insert(script_hash, sealed_key.clone());

        self.save_metadata()?;

        info!("Imported key to TEE wallet: {}", sealed_key.address());
        Ok(sealed_key)
    }

    /// Get all keys in wallet
    pub fn list_keys(&self) -> Vec<SealedKey> {
        self.keys.read().values().cloned().collect()
    }

    /// Get key by script hash
    pub fn get_key(&self, script_hash: &[u8; 20]) -> Option<SealedKey> {
        self.keys.read().get(script_hash).cloned()
    }

    /// Get the default account
    pub fn default_account(&self) -> Option<SealedKey> {
        self.default_account
            .read()
            .and_then(|hash| self.keys.read().get(&hash).cloned())
    }

    /// Set the default account
    pub fn set_default_account(&self, script_hash: &[u8; 20]) -> TeeResult<()> {
        if !self.keys.read().contains_key(script_hash) {
            return Err(TeeError::KeyNotFound(hex::encode(script_hash)));
        }
        *self.default_account.write() = Some(*script_hash);
        self.save_metadata()?;
        Ok(())
    }

    /// Sign data with a key
    pub fn sign(&self, script_hash: &[u8; 20], data: &[u8]) -> TeeResult<Vec<u8>> {
        if self.is_locked() {
            return Err(TeeError::Other("Wallet is locked".to_string()));
        }

        let sealed_key = self
            .keys
            .read()
            .get(script_hash)
            .cloned()
            .ok_or_else(|| TeeError::KeyNotFound(hex::encode(script_hash)))?;

        // Unseal the private key inside TEE
        let private_key = sealed_key.unseal(&self.enclave)?;

        let signature = self.sign_with_key(&private_key, data)?;

        Ok(signature)
    }

    /// Delete a key from wallet
    pub fn delete_key(&self, script_hash: &[u8; 20]) -> TeeResult<()> {
        if self.is_locked() {
            return Err(TeeError::Other("Wallet is locked".to_string()));
        }

        let removed = self.keys.write().remove(script_hash);
        if removed.is_none() {
            return Err(TeeError::KeyNotFound(hex::encode(script_hash)));
        }

        // Delete key file
        let key_filename = format!("key_{}.json", hex::encode(&script_hash[..8]));
        let key_path = self.path.join(&key_filename);
        if key_path.exists() {
            std::fs::remove_file(&key_path)?;
        }

        // Update default account if needed
        if self.default_account.read().as_ref() == Some(script_hash) {
            *self.default_account.write() = self.keys.read().keys().next().copied();
        }

        self.save_metadata()?;
        info!("Deleted key from TEE wallet: {}", hex::encode(script_hash));
        Ok(())
    }

    fn save_metadata(&self) -> TeeResult<()> {
        let keys: Vec<String> = self
            .keys
            .read()
            .keys()
            .map(|hash| format!("key_{}.json", hex::encode(&hash[..8])))
            .collect();

        let metadata = WalletMetadata {
            name: self.name.clone(),
            version: "1.0.0".to_string(),
            default_account: self.default_account().map(|k| k.address()),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            keys,
        };

        let metadata_path = self.path.join("wallet.json");
        let json = serde_json::to_string_pretty(&metadata)?;
        // Write with restrictive permissions (owner read/write only)
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            std::fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&metadata_path)
                .and_then(|mut f| std::io::Write::write_all(&mut f, json.as_bytes()))?;
        }
        #[cfg(not(unix))]
        {
            std::fs::write(&metadata_path, json)?;
        }
        Ok(())
    }

    /// Derive compressed secp256r1 public key from a 32-byte private key.
    fn derive_public_key(&self, private_key: &[u8; 32]) -> TeeResult<Vec<u8>> {
        Secp256r1Crypto::derive_public_key(private_key)
            .map_err(|e| TeeError::Other(format!("Failed to derive public key: {e}")))
    }

    /// Compute Neo N3 script hash for a signature contract from compressed public key.
    fn compute_script_hash(&self, public_key: &[u8]) -> TeeResult<[u8; 20]> {
        // Script: PUSHDATA(public_key) + SYSCALL(CheckSig)
        let mut script = Vec::new();
        script.push(0x0c); // PUSHDATA1
        script.push(public_key.len() as u8);
        script.extend_from_slice(public_key);
        script.push(0x41); // SYSCALL
        let syscall_hash = Crypto::sha256(b"System.Crypto.CheckSig");
        script.extend_from_slice(&syscall_hash[..4]);

        Ok(Crypto::hash160(&script))
    }

    /// Sign data with a secp256r1 private key (returns 64-byte signature).
    fn sign_with_key(&self, private_key: &[u8], data: &[u8]) -> TeeResult<Vec<u8>> {
        let key: &[u8; 32] = private_key.try_into().map_err(|_| {
            TeeError::Other(format!(
                "Invalid private key length: expected 32, got {}",
                private_key.len()
            ))
        })?;

        Secp256r1Crypto::sign(data, key)
            .map(|sig| sig.to_vec())
            .map_err(|e| TeeError::Other(format!("Failed to sign: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclave::EnclaveConfig;
    use tempfile::tempdir;

    fn setup_enclave() -> (tempfile::TempDir, Arc<TeeEnclave>) {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().join("enclave"),
            simulation: true,
            ..Default::default()
        };
        let enclave = Arc::new(TeeEnclave::new(config));
        enclave.initialize().unwrap();
        (temp, enclave)
    }

    #[test]
    fn test_create_wallet() {
        let (temp, enclave) = setup_enclave();
        let wallet_path = temp.path().join("wallet");

        let wallet = TeeWallet::create(enclave, "test-wallet", &wallet_path).unwrap();
        assert_eq!(wallet.name(), "test-wallet");
        assert!(wallet.list_keys().is_empty());
    }

    #[test]
    fn test_create_and_list_keys() {
        let (temp, enclave) = setup_enclave();
        let wallet_path = temp.path().join("wallet");

        let wallet = TeeWallet::create(enclave, "test-wallet", &wallet_path).unwrap();

        let key1 = wallet.create_key(Some("key1".to_string())).unwrap();
        let _key2 = wallet.create_key(Some("key2".to_string())).unwrap();

        let keys = wallet.list_keys();
        assert_eq!(keys.len(), 2);

        // First key should be default
        assert_eq!(
            wallet.default_account().unwrap().script_hash,
            key1.script_hash
        );
    }

    #[test]
    fn test_sign_with_key() {
        let (temp, enclave) = setup_enclave();
        let wallet_path = temp.path().join("wallet");

        let wallet = TeeWallet::create(enclave, "test-wallet", &wallet_path).unwrap();
        let key = wallet.create_key(None).unwrap();

        let data = b"test data to sign";
        let signature = wallet.sign(&key.script_hash, data).unwrap();

        assert!(!signature.is_empty());
    }
}
