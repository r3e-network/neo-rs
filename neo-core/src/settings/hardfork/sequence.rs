use alloc::collections::BTreeMap;

use crate::settings::error::ProtocolSettingsError;

use super::model::Hardfork;

pub(crate) fn ensure_hardfork_defaults(map: &mut BTreeMap<Hardfork, u32>) {
    for hardfork in Hardfork::ALL {
        if let Some(entry) = map.get_mut(&hardfork) {
            if *entry == u32::MAX {
                *entry = 0;
            } else {
                break;
            }
        }
    }
}

pub(crate) fn validate_hardfork_sequence(
    map: &BTreeMap<Hardfork, u32>,
) -> Result<(), ProtocolSettingsError> {
    let mut prev: Option<(Hardfork, u32)> = None;
    for (&hardfork, &height) in map.iter() {
        if height == u32::MAX {
            continue;
        }
        if let Some((prev_hf, prev_height)) = prev {
            if (hardfork as i32) - (prev_hf as i32) > 1 {
                return Err(ProtocolSettingsError::HardforkGap {
                    current: prev_hf,
                    next: hardfork,
                });
            }
            if height < prev_height {
                return Err(ProtocolSettingsError::HardforkHeightRegression {
                    current: prev_hf,
                    current_height: prev_height,
                    next: hardfork,
                    next_height: height,
                });
            }
        }
        prev = Some((hardfork, height));
    }
    Ok(())
}
