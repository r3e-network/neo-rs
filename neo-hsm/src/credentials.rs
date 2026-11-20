//! Injectable credential acquisition for hardware authentication.
//!
//! Phase 9 (F15): the runtime no longer reads a terminal directly. Everything
//! that needs operator input during authentication goes through
//! [`CredentialProvider`], so:
//!
//! * node CLI adapters use the default [`ConsoleCredentialProvider`] (stdin /
//!   `rpassword` prompt, unchanged behavior), and
//! * library consumers (HSM-backed signer services, remote orchestrators,
//!   tests) inject their own implementation — a pre-shared PIN source, a
//!   hardware pinpad bridge, or a stub.
//!
//! PIN material is returned as [`Zeroizing<String>`] so it is cleared on drop,
//! exactly as the console prompt already did.

use crate::error::HsmResult;
use crate::pin::prompt_pin;
use std::io::{self, Write};
use zeroize::Zeroizing;

/// Source of the operator input required to authenticate a hardware device.
///
/// Implementations must be cheap to call repeatedly: the runtime retries an
/// invalid PIN up to `HsmConfig::max_pin_attempts` times by calling
/// [`request_pin`](CredentialProvider::request_pin) again.
pub trait CredentialProvider: Send + Sync {
    /// Requests the unlock PIN for the device described by `device_name`.
    ///
    /// Returning [`crate::HsmError::PinRequired`] reports an empty/absent PIN;
    /// the runtime treats it like any other failed attempt.
    fn request_pin(&self, device_name: &str) -> HsmResult<Zeroizing<String>>;

    /// Waits for the operator to complete a physical confirmation step.
    ///
    /// Used by transports that unlock on the device itself (Ledger): the
    /// implementation blocks until the operator acknowledges, or fails when no
    /// interactive channel exists.
    fn confirm_device_unlock(&self, device_name: &str) -> HsmResult<()>;
}

/// Terminal-backed [`CredentialProvider`].
///
/// This is the historical behavior: a prompt on stdout, the PIN read without
/// echo through `rpassword`, and a blocking "press Enter" confirmation. The
/// node CLI adapter uses it; it is never referenced by the crate internals.
#[derive(Debug, Default, Clone, Copy)]
pub struct ConsoleCredentialProvider;

impl ConsoleCredentialProvider {
    /// Creates the console provider.
    pub fn new() -> Self {
        Self
    }
}

impl CredentialProvider for ConsoleCredentialProvider {
    fn request_pin(&self, device_name: &str) -> HsmResult<Zeroizing<String>> {
        prompt_pin(device_name)
    }

    fn confirm_device_unlock(&self, device_name: &str) -> HsmResult<()> {
        println!("Unlock the {device_name} device and open the Neo app, then press Enter.");
        io::stdout().flush()?;
        let mut input = String::new();
        io::stdin().read_line(&mut input)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::HsmError;

    /// Proves the injection surface: a library consumer supplies the PIN and
    /// never touches stdin.
    struct FixedPinProvider {
        pin: &'static str,
    }

    impl CredentialProvider for FixedPinProvider {
        fn request_pin(&self, _device_name: &str) -> HsmResult<Zeroizing<String>> {
            if self.pin.is_empty() {
                return Err(HsmError::PinRequired);
            }
            Ok(Zeroizing::new(self.pin.to_string()))
        }

        fn confirm_device_unlock(&self, _device_name: &str) -> HsmResult<()> {
            Ok(())
        }
    }

    #[test]
    fn injected_provider_supplies_the_pin_without_console_interaction() {
        let provider: Box<dyn CredentialProvider> = Box::new(FixedPinProvider { pin: "1234" });
        let pin = provider.request_pin("SoftHSM").expect("injected PIN");
        assert_eq!(pin.as_str(), "1234");
        provider
            .confirm_device_unlock("Ledger")
            .expect("injected confirmation");
    }

    #[test]
    fn empty_injected_pin_reports_pin_required() {
        let provider = FixedPinProvider { pin: "" };
        assert!(matches!(
            provider.request_pin("SoftHSM"),
            Err(HsmError::PinRequired)
        ));
    }
}
