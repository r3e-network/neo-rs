use alloc::string::String;
use core::str::FromStr;

use super::super::{WitnessScope, WitnessScopes};

impl FromStr for WitnessScopes {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("None") {
            return Ok(WitnessScopes::new());
        }
        if trimmed.eq_ignore_ascii_case("Global") {
            return Ok(WitnessScopes::from_bits(WitnessScope::Global as u8));
        }

        let mut bits = 0u8;
        for part in trimmed.split(|c| c == '|' || c == ',') {
            let name = part.trim();
            if name.is_empty() {
                continue;
            }
            let scope = WitnessScope::from_str(name)?;
            if scope == WitnessScope::Global {
                return Err("Global scope cannot be combined with other scopes".to_string());
            }
            bits |= scope as u8;
        }
        if bits == 0 {
            return Ok(WitnessScopes::new());
        }
        Ok(WitnessScopes::from_bits(bits))
    }
}
