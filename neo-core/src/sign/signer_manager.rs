// Copyright (C) 2015-2025 The Neo Project.
//
// signer_manager.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::i_signer::ISigner;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

/// Global registry of signers
static SIGNERS: LazyLock<RwLock<HashMap<String, Arc<dyn ISigner>>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));

/// Manages signers for the Neo blockchain
pub struct SignerManager;

impl SignerManager {
    /// Get a signer by name. If only one signer is registered, it will return the only one signer.
    ///
    /// # Arguments
    /// * `name` - The name of the signer
    ///
    /// # Returns
    /// The signer; `None` if not found or no signer or multiple signers are registered.
    pub fn get_signer_or_default(name: &str) -> Option<Arc<dyn ISigner>> {
        let signers = SIGNERS.read();

        if !name.is_empty() {
            if let Some(signer) = signers.get(name) {
                return Some(signer.clone());
            }
        }

        if signers.len() == 1 {
            return signers.values().next().cloned();
        }

        None
    }

    /// Register a signer, and it only can be called before the node starts.
    ///
    /// # Arguments
    /// * `name` - The name of the signer
    /// * `signer` - The signer to register
    ///
    /// # Errors
    /// Returns error when name is null or empty, or when name is already registered
    pub fn register_signer(name: String, signer: Arc<dyn ISigner>) -> Result<(), String> {
        if name.is_empty() {
            return Err("Name cannot be null or empty".to_string());
        }

        let mut signers = SIGNERS.write();

        if signers.contains_key(&name) {
            return Err(format!("Signer {} already exists", name));
        }

        signers.insert(name, signer);
        Ok(())
    }

    /// Unregister a signer, and it only can be called before the node starts.
    ///
    /// # Arguments
    /// * `name` - The name of the signer
    ///
    /// # Returns
    /// `true` if the signer is unregistered; otherwise, `false`.
    pub fn unregister_signer(name: &str) -> bool {
        if name.is_empty() {
            return false;
        }

        let mut signers = SIGNERS.write();
        signers.remove(name).is_some()
    }
}
