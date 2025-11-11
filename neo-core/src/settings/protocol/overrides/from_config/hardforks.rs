use alloc::collections::BTreeMap;
use core::str::FromStr;

use crate::settings::{
    error::ProtocolSettingsError,
    hardfork::{self, Hardfork},
};

pub(super) fn parse_hardforks(
    entries: Option<BTreeMap<String, u32>>,
) -> Result<Option<BTreeMap<Hardfork, u32>>, ProtocolSettingsError> {
    let Some(list) = entries else {
        return Ok(None);
    };
    let mut hardforks = hardfork::build_hardfork_map(&[]);
    for (name, height) in list {
        let hardfork = Hardfork::from_str(&name)?;
        hardforks.insert(hardfork, height);
    }
    hardfork::ensure_hardfork_defaults(&mut hardforks);
    hardfork::validate_hardfork_sequence(&hardforks)?;
    Ok(Some(hardforks))
}
