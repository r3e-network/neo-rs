//! Interactive PIN prompt using rpassword

use crate::error::{HsmError, HsmResult};
use std::io::{self, Write};

/// Prompt user for HSM PIN at startup
///
/// # Arguments
/// * `device_name` - Name of the HSM device to display in prompt
///
/// # Returns
/// The PIN entered by the user
pub fn prompt_pin(device_name: &str) -> HsmResult<String> {
    print!("Enter PIN for HSM device '{}': ", device_name);
    io::stdout().flush()?;

    let pin = rpassword::read_password()
        .map_err(|e| HsmError::Other(format!("Failed to read PIN: {}", e)))?;

    if pin.is_empty() {
        return Err(HsmError::PinRequired);
    }

    Ok(pin)
}

/// Prompt for PIN with retry logic
///
/// # Arguments
/// * `device_name` - Name of the HSM device to display in prompt
/// * `max_attempts` - Maximum number of PIN entry attempts
///
/// # Returns
/// The PIN entered by the user, or error after max attempts
pub fn prompt_pin_with_retry(device_name: &str, max_attempts: u32) -> HsmResult<String> {
    for attempt in 1..=max_attempts {
        match prompt_pin(device_name) {
            Ok(pin) => return Ok(pin),
            Err(HsmError::PinRequired) if attempt < max_attempts => {
                eprintln!("PIN cannot be empty. Attempt {}/{}", attempt, max_attempts);
                continue;
            }
            Err(e) => return Err(e),
        }
    }
    Err(HsmError::PinRequired)
}

#[cfg(test)]
mod tests {
    // Interactive PIN prompts require manual testing.
}
