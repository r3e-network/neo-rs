//! HSM (Hardware Security Module) integration for neo-node
//!
//! This module provides HSM support for wallet signing operations.
//! It supports Ledger hardware wallets and PKCS#11 generic HSM interfaces.

use anyhow::{Context, Result};
use neo_hsm::{HsmConfig, HsmDeviceInfo, HsmDeviceType, HsmKeyInfo, HsmSigner};
use std::sync::Arc;
use tracing::{info, warn};

use crate::cli::NodeCli;

/// HSM runtime state
pub struct HsmRuntime {
    /// The HSM signer instance
    pub signer: Arc<dyn HsmSigner>,
    /// HSM configuration
    pub config: HsmConfig,
    /// Cached key info (if a specific key was requested)
    pub active_key: Option<HsmKeyInfo>,
    /// Address version for the current network
    pub address_version: u8,
}

impl HsmRuntime {
    /// Get the device info
    pub fn device_info(&self) -> &HsmDeviceInfo {
        self.signer.device_info()
    }

    /// Check if HSM is ready
    pub fn is_ready(&self) -> bool {
        self.signer.is_ready()
    }

    /// Get the active key info
    pub fn active_key(&self) -> Option<&HsmKeyInfo> {
        self.active_key.as_ref()
    }
}

/// Initialize HSM from CLI arguments
pub async fn initialize_hsm(cli: &NodeCli, address_version: u8) -> Result<HsmRuntime> {
    let device_type: HsmDeviceType = cli
        .hsm_device
        .parse()
        .map_err(|e: String| anyhow::anyhow!(e))?;

    info!(
        target: "neo::hsm",
        "Initializing HSM with device type: {}",
        device_type
    );

    let config = HsmConfig {
        device_type,
        slot: cli.hsm_slot,
        key_id: cli.hsm_key_id.clone(),
        pkcs11_lib: cli.hsm_pkcs11_lib.clone(),
        skip_pin: cli.hsm_no_pin,
        ..Default::default()
    };

    let signer: Arc<dyn HsmSigner> = match device_type {
        HsmDeviceType::Simulation => {
            #[cfg(feature = "hsm")]
            {
                let sim = neo_hsm::SimulationSigner::with_test_key()
                    .context("Failed to create simulation signer")?;
                Arc::new(sim)
            }
            #[cfg(not(feature = "hsm"))]
            {
                anyhow::bail!("HSM feature not enabled");
            }
        }
        HsmDeviceType::Ledger => {
            #[cfg(feature = "hsm-ledger")]
            {
                let ledger = neo_hsm::LedgerSigner::new(cli.hsm_slot as u32)
                    .context("Failed to initialize Ledger device")?;
                Arc::new(ledger)
            }
            #[cfg(not(feature = "hsm-ledger"))]
            {
                anyhow::bail!("Ledger support not enabled. Build with --features hsm-ledger");
            }
        }
        HsmDeviceType::Pkcs11 => {
            #[cfg(feature = "hsm-pkcs11")]
            {
                let lib_path = cli.hsm_pkcs11_lib.as_ref().ok_or_else(|| {
                    anyhow::anyhow!("PKCS#11 library path required (--hsm-pkcs11-lib)")
                })?;
                let pkcs11 = neo_hsm::Pkcs11Signer::new(lib_path, cli.hsm_slot)
                    .context("Failed to initialize PKCS#11 device")?;
                Arc::new(pkcs11)
            }
            #[cfg(not(feature = "hsm-pkcs11"))]
            {
                anyhow::bail!("PKCS#11 support not enabled. Build with --features hsm-pkcs11");
            }
        }
    };

    // Prompt for PIN if required
    let device_info = signer.device_info();
    if device_info.requires_pin && !config.skip_pin {
        let device_name = format!("{} {}", device_info.manufacturer, device_info.model);

        match device_type {
            HsmDeviceType::Ledger => {
                // Ledger handles PIN on-device
                println!();
                println!("Please unlock your Ledger device and open the Neo app.");
                println!("Press Enter when ready...");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
            }
            _ => {
                // Prompt for PIN
                let pin = neo_hsm::prompt_pin(&device_name).context("Failed to read PIN")?;
                signer.unlock(&pin).await.context("Failed to unlock HSM")?;
            }
        }
    }

    // Unlock the device (for Ledger, this verifies the Neo app is open)
    if !signer.is_ready() {
        signer
            .unlock("")
            .await
            .context("Failed to initialize HSM")?;
    }

    // Get active key if specified
    let active_key = if let Some(ref key_id) = config.key_id {
        Some(signer.get_key(key_id).await.context("Failed to get key")?)
    } else {
        // Try to get the default key
        match signer.list_keys().await {
            Ok(keys) if !keys.is_empty() => {
                let key = keys.into_iter().next();
                if let Some(ref k) = key {
                    info!(
                        target: "neo::hsm",
                        "Using default key: {} ({})",
                        k.key_id,
                        k.neo_address(address_version)
                    );
                }
                key
            }
            _ => None,
        }
    };

    let device_info = signer.device_info();
    info!(
        target: "neo::hsm",
        "HSM initialized: {} {} ({})",
        device_info.manufacturer,
        device_info.model,
        device_type
    );

    if let Some(ref key) = active_key {
        info!(
            target: "neo::hsm",
            "Active key: {} -> {}",
            key.key_id,
            key.neo_address(address_version)
        );
    }

    Ok(HsmRuntime {
        signer,
        config,
        active_key,
        address_version,
    })
}

/// Print HSM status information
pub fn print_hsm_status(runtime: &HsmRuntime) {
    let info = runtime.device_info();
    println!("HSM Status:");
    println!("  Device: {} {}", info.manufacturer, info.model);
    println!("  Type: {}", info.device_type);
    println!("  Connected: {}", info.is_connected);
    println!("  Ready: {}", runtime.is_ready());

    if let Some(ref serial) = info.serial_number {
        println!("  Serial: {}", serial);
    }
    if let Some(ref version) = info.firmware_version {
        println!("  Firmware: {}", version);
    }

    if let Some(ref key) = runtime.active_key {
        println!("  Active Key: {}", key.key_id);
        println!("  Address: {}", key.neo_address(runtime.address_version));
    }
}
