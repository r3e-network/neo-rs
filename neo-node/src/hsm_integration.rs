//! HSM runtime construction and hardware-only initialization.
//!
//! No software signer is accepted as an HSM replacement. Missing features,
//! missing devices, authentication failures, and key discovery failures all
//! abort node wallet initialization instead of falling back to host keys.

use crate::cli::NodeCli;
use crate::hsm::{
    HsmConfig, HsmDeviceInfo, HsmDeviceType, HsmError, HsmKeyInfo, HsmSigner, prompt_pin,
};
use anyhow::{Context, Result, anyhow, bail};
use std::future::Future;
use std::sync::Arc;
use tracing::info;

/// Initialized HSM state shared with the wallet adapter.
pub struct HsmRuntime {
    /// Authenticated hardware signer.
    pub signer: Arc<dyn HsmSigner>,
    /// Key explicitly selected or discovered after device authentication.
    pub active_key: HsmKeyInfo,
    /// Address version for the active network.
    pub address_version: u8,
}

impl HsmRuntime {
    /// Returns information reported by the connected hardware device.
    pub fn device_info(&self) -> &HsmDeviceInfo {
        self.signer.device_info()
    }

    /// Returns whether the hardware signer completed authentication.
    pub fn is_ready(&self) -> bool {
        self.signer.is_ready()
    }
}

/// Initializes an HSM from command-line configuration.
pub async fn initialize_hsm(cli: &NodeCli, address_version: u8) -> Result<HsmRuntime> {
    let device_type: HsmDeviceType = cli
        .hsm_device
        .parse()
        .map_err(|error: String| anyhow!(error))?;
    let config = HsmConfig {
        device_type,
        slot: cli.hsm_slot,
        key_id: cli.hsm_key_id.clone(),
        pkcs11_lib: cli.hsm_pkcs11_lib.clone(),
        skip_pin: cli.hsm_no_pin,
        ..HsmConfig::default()
    };
    initialize_with_config(config, address_version).await
}

async fn initialize_with_config(config: HsmConfig, address_version: u8) -> Result<HsmRuntime> {
    if config.max_pin_attempts == 0 {
        bail!("HSM max_pin_attempts must be greater than zero");
    }
    if config.timeout_ms == 0 {
        bail!("HSM timeout_ms must be greater than zero");
    }
    if config.device_type == HsmDeviceType::Pkcs11 && config.skip_pin {
        bail!("PKCS#11 PIN authentication cannot be skipped");
    }

    info!(
        target: "neo::hsm",
        device = %config.device_type,
        slot = config.slot,
        "Initializing hardware wallet"
    );

    let signer: Arc<dyn HsmSigner> = match config.device_type {
        HsmDeviceType::Ledger => {
            #[cfg(feature = "hsm-ledger")]
            {
                let device_index = u32::try_from(config.slot)
                    .context("Ledger device index exceeds the supported range")?;
                Arc::new(
                    crate::hsm::LedgerSigner::new_with_timeout(device_index, config.timeout_ms)
                        .context("failed to initialize Ledger device")?,
                )
            }
            #[cfg(not(feature = "hsm-ledger"))]
            {
                bail!("Ledger support is not enabled; rebuild with --features hsm-ledger");
            }
        }
        HsmDeviceType::Pkcs11 => {
            #[cfg(feature = "hsm-pkcs11")]
            {
                let library_path = config.pkcs11_lib.as_deref().ok_or_else(|| {
                    anyhow!("PKCS#11 library path is required (--hsm-pkcs11-lib)")
                })?;
                Arc::new(
                    crate::hsm::Pkcs11Signer::new(library_path, config.slot)
                        .context("failed to initialize PKCS#11 device")?,
                )
            }
            #[cfg(not(feature = "hsm-pkcs11"))]
            {
                bail!("PKCS#11 support is not enabled; rebuild with --features hsm-pkcs11");
            }
        }
    };

    authenticate(&*signer, &config).await?;
    if !signer.is_ready() {
        bail!("HSM authentication completed without a ready device");
    }
    if !signer
        .verify_device()
        .await
        .context("failed to verify HSM device")?
    {
        bail!("HSM device verification failed");
    }

    let active_key = match config.key_id.as_deref() {
        Some(key_id) => signer
            .get_key(key_id)
            .await
            .with_context(|| format!("failed to load HSM key {key_id}"))?,
        None => {
            let key_id = signer
                .default_key_id()
                .await
                .context("HSM did not provide a usable default key")?;
            signer
                .get_key(&key_id)
                .await
                .with_context(|| format!("failed to load HSM key {key_id}"))?
        }
    };

    info!(
        target: "neo::hsm",
        device = %config.device_type,
        key_id = %active_key.key_id,
        address = %active_key.neo_address(address_version),
        "Hardware wallet initialized"
    );

    Ok(HsmRuntime {
        signer,
        active_key,
        address_version,
    })
}

async fn authenticate(signer: &dyn HsmSigner, config: &HsmConfig) -> Result<()> {
    match config.device_type {
        HsmDeviceType::Ledger => {
            if !config.skip_pin {
                println!("Unlock the Ledger device and open the Neo app, then press Enter.");
                let mut input = String::new();
                std::io::stdin()
                    .read_line(&mut input)
                    .context("failed to read Ledger confirmation")?;
            }
            signer
                .unlock("")
                .await
                .context("Ledger is not unlocked or the Neo app is not open")?;
        }
        HsmDeviceType::Pkcs11 => {
            let device = signer.device_info();
            let device_name = format!("{} {}", device.manufacturer, device.model);
            let mut last_error = None;
            for attempt in 1..=config.max_pin_attempts {
                let pin = prompt_pin(&device_name).context("failed to read PKCS#11 PIN")?;
                match signer.unlock(pin.as_str()).await {
                    Ok(()) => return Ok(()),
                    Err(HsmError::InvalidPin) if attempt < config.max_pin_attempts => {
                        eprintln!("Invalid PIN. Attempt {attempt}/{}", config.max_pin_attempts);
                        last_error = Some(HsmError::InvalidPin);
                    }
                    Err(error) => return Err(error).context("failed to unlock PKCS#11 device"),
                }
            }
            return Err(last_error.unwrap_or(HsmError::PinRequired))
                .context("PKCS#11 authentication attempts exhausted");
        }
    }
    Ok(())
}

/// Runs an async hardware operation on a dedicated thread.
///
/// The public wallet interface is synchronous and may be called either inside
/// or outside Tokio. A dedicated thread avoids nested-runtime and
/// `block_in_place` panics while keeping blocking USB/PKCS#11 work off the
/// caller's async executor thread.
pub(crate) fn run_hardware_operation<F>(future: F) -> Result<F::Output>
where
    F: Future + Send,
    F::Output: Send,
{
    std::thread::scope(|scope| {
        scope
            .spawn(|| futures::executor::block_on(future))
            .join()
            .map_err(|_| anyhow!("HSM operation thread panicked"))
    })
}

/// Prints non-secret HSM status information.
pub fn print_hsm_status(runtime: &HsmRuntime) {
    let info = runtime.device_info();
    println!("HSM Status:");
    println!("  Device: {} {}", info.manufacturer, info.model);
    println!("  Type: {}", info.device_type);
    println!("  Connected: {}", info.is_connected);
    println!("  Ready: {}", runtime.is_ready());
    println!("  Active Key: {}", runtime.active_key.key_id);
    println!(
        "  Address: {}",
        runtime.active_key.neo_address(runtime.address_version)
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn pkcs11_pin_bypass_is_rejected_before_loading_a_library() {
        let config = HsmConfig::pkcs11(
            std::path::PathBuf::from("this-library-must-not-be-loaded.dll"),
            0,
        )
        .with_skip_pin(true);

        let error = initialize_with_config(config, 0x35)
            .await
            .err()
            .expect("PIN bypass must fail closed");
        assert!(error.to_string().contains("cannot be skipped"));
    }

    #[cfg(not(feature = "hsm-ledger"))]
    #[tokio::test]
    async fn ledger_feature_is_required_without_software_fallback() {
        let error = initialize_with_config(HsmConfig::ledger(0), 0x35)
            .await
            .err()
            .expect("missing Ledger feature must fail closed");
        assert!(error.to_string().contains("hsm-ledger"));
    }

    #[test]
    fn hardware_operation_runs_inside_current_thread_tokio_runtime() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        runtime.block_on(async {
            let value = run_hardware_operation(async { 42 }).expect("hardware operation");
            assert_eq!(value, 42);
        });
    }
}
