use alloc::collections::BTreeMap;

use crate::settings::{
    error::ProtocolSettingsError,
    hardfork::{self, Hardfork},
    ProtocolSettings,
};

pub(super) fn apply_hardfork_overrides(
    settings: &mut ProtocolSettings,
    overrides: Option<BTreeMap<Hardfork, u32>>,
) -> Result<(), ProtocolSettingsError> {
    if let Some(mut hardforks) = overrides {
        hardfork::ensure_hardfork_defaults(&mut hardforks);
        hardfork::validate_hardfork_sequence(&hardforks)?;
        settings.hardforks = hardforks;
    } else {
        hardfork::ensure_hardfork_defaults(&mut settings.hardforks);
        hardfork::validate_hardfork_sequence(&settings.hardforks)?;
    }
    Ok(())
}
