use super::ProtocolSettings;
use crate::settings::{
    config::ProtocolSettingsConfig,
    error::ProtocolSettingsError,
    protocol::{
        defaults::{build_settings, MAINNET, PRIVATENET, TESTNET},
        overrides::ProtocolSettingsOverrides,
        settings::apply::apply_overrides,
    },
};

impl ProtocolSettings {
    pub fn mainnet() -> Self {
        build_settings(&MAINNET)
    }

    pub fn testnet() -> Self {
        build_settings(&TESTNET)
    }

    pub fn privatenet() -> Self {
        build_settings(&PRIVATENET)
    }

    pub fn with_overrides(
        mut self,
        overrides: ProtocolSettingsOverrides,
    ) -> Result<Self, ProtocolSettingsError> {
        apply_overrides(&mut self, overrides)?;
        Ok(self)
    }

    pub fn with_config(
        self,
        config: ProtocolSettingsConfig,
    ) -> Result<Self, ProtocolSettingsError> {
        let overrides = ProtocolSettingsOverrides::try_from(config)?;
        self.with_overrides(overrides)
    }
}

impl Default for ProtocolSettings {
    fn default() -> Self {
        Self::mainnet()
    }
}
