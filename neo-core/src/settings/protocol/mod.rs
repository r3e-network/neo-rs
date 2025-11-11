mod defaults;
mod overrides;
mod settings;

pub use overrides::ProtocolSettingsOverrides;
pub use settings::ProtocolSettings;

pub fn calculate_max_valid_until_block_increment(milliseconds_per_block: u32) -> u32 {
    const DAY_MS: u32 = 86_400_000;
    let per_block = milliseconds_per_block.max(1);
    let increment = DAY_MS / per_block;
    increment.max(1)
}
