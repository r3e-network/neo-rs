//! Interactive PIN prompt using rpassword

use crate::error::{HsmError, HsmResult};
use std::io::{self, Write};
use zeroize::Zeroizing;

/// Prompt user for HSM PIN at startup.
///
/// Returns a `Zeroizing<String>` so PIN material is cleared on drop.
pub fn prompt_pin(device_name: &str) -> HsmResult<Zeroizing<String>> {
    print!("Enter PIN for HSM device '{device_name}': ");
    io::stdout().flush()?;

    let pin = rpassword::read_password()
        .map_err(|e| HsmError::Other(format!("Failed to read PIN: {e}")))?;

    if pin.is_empty() {
        return Err(HsmError::PinRequired);
    }

    Ok(Zeroizing::new(pin))
}

/// Prompt for PIN with retry logic.
pub fn prompt_pin_with_retry(
    device_name: &str,
    max_attempts: u32,
) -> HsmResult<Zeroizing<String>> {
    for attempt in 1..=max_attempts {
        match prompt_pin(device_name) {
            Ok(pin) => return Ok(pin),
            Err(HsmError::PinRequired) if attempt < max_attempts => {
                eprintln!("PIN cannot be empty. Attempt {attempt}/{max_attempts}");
            }
            Err(e) if attempt < max_attempts => {
                eprintln!("PIN entry failed: {e}. Attempt {attempt}/{max_attempts}");
            }
            Err(e) => return Err(e),
        }
    }
    Err(HsmError::PinRequired)
}
