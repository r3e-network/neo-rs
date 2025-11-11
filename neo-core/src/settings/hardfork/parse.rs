use alloc::collections::BTreeMap;

use super::model::Hardfork;

pub(crate) fn build_hardfork_map(entries: &[(Hardfork, u32)]) -> BTreeMap<Hardfork, u32> {
    let mut map = BTreeMap::new();
    for hardfork in Hardfork::ALL {
        map.insert(hardfork, u32::MAX);
    }
    for (hf, height) in entries {
        map.insert(*hf, *height);
    }
    map
}
