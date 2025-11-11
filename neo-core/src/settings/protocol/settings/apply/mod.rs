mod committee;
mod fields;
mod hardforks;

use crate::settings::error::ProtocolSettingsError;

use super::super::{
    calculate_max_valid_until_block_increment, overrides::ProtocolSettingsOverrides,
    ProtocolSettings,
};

pub(super) fn apply_overrides(
    settings: &mut ProtocolSettings,
    overrides: ProtocolSettingsOverrides,
) -> Result<(), ProtocolSettingsError> {
    fields::apply_simple_fields(
        settings,
        overrides.network_magic,
        overrides.address_version,
        overrides.seed_list,
        overrides.milliseconds_per_block,
        overrides.max_transactions_per_block,
        overrides.memory_pool_max_transactions,
        overrides.max_traceable_blocks,
        overrides.max_valid_until_block_increment,
        overrides.scrypt,
        overrides.initial_gas_distribution,
        calculate_max_valid_until_block_increment,
    );

    committee::apply_committee_overrides(
        settings,
        overrides.standby_committee,
        overrides.validators_count,
    )?;

    hardforks::apply_hardfork_overrides(settings, overrides.hardforks)?;

    Ok(())
}
