mod codec;
mod display;

use super::WitnessScope;

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
pub struct WitnessScopes {
    scopes: u8,
}

impl WitnessScopes {
    pub fn new() -> Self {
        Self {
            scopes: WitnessScope::None as u8,
        }
    }

    pub fn from_bits(bits: u8) -> Self {
        Self { scopes: bits }
    }

    pub fn scopes(&self) -> u8 {
        self.scopes
    }

    pub fn add_scope(&mut self, scope: WitnessScope) {
        self.scopes |= scope as u8;
    }

    pub fn has_scope(&self, scope: WitnessScope) -> bool {
        self.scopes & (scope as u8) != 0
    }

    pub fn bits(&self) -> u8 {
        self.scopes
    }
}

impl Default for WitnessScopes {
    fn default() -> Self {
        Self::new()
    }
}
