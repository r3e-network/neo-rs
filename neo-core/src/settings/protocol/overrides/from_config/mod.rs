mod committee;
mod converters;
mod hardforks;

use crate::settings::config::ProtocolSettingsConfig;
use crate::settings::error::ProtocolSettingsError;

use super::model::ProtocolSettingsOverrides;

impl TryFrom<ProtocolSettingsConfig> for ProtocolSettingsOverrides {
    type Error = ProtocolSettingsError;

    fn try_from(config: ProtocolSettingsConfig) -> Result<Self, Self::Error> {
        let mut overrides = ProtocolSettingsOverrides::default();
        converters::apply_numeric_overrides(&config, &mut overrides);
        overrides.seed_list = converters::normalize_seed_list(config.seed_list);
        overrides.initial_gas_distribution = config.initial_gas_distribution;

        overrides.standby_committee =
            committee::parse_committee(config.standby_committee.as_deref())?;
        overrides.scrypt = config.scrypt.map(TryInto::try_into).transpose()?;
        overrides.hardforks = hardforks::parse_hardforks(config.hardforks)?;

        Ok(overrides)
    }
}
