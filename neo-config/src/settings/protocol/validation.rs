//! Hardfork sequence validation for `ProtocolSettings`.
//!
//! C# fills omitted leading hardforks with height zero and requires explicitly
//! configured hardforks to be sequential and non-decreasing. Keeping those rules
//! here leaves the protocol root as a typed settings facade.

use std::collections::{HashMap, hash_map::Entry};

use crate::hardfork::{Hardfork, HardforkManager};

use super::{ProtocolConfigError, ProtocolSettings};

impl ProtocolSettings {
    /// Ensures omitted hardforks are included.
    /// Matches C# EnsureOmmitedHardforks method
    pub(super) fn ensure_omitted_hardforks(
        hardforks: HashMap<Hardfork, u32>,
    ) -> HashMap<Hardfork, u32> {
        let mut hardforks = hardforks;
        let mut encountered_configured = false;
        for hardfork in HardforkManager::all() {
            match hardforks.entry(hardfork) {
                Entry::Occupied(_) => encountered_configured = true,
                Entry::Vacant(entry) if !encountered_configured => {
                    entry.insert(0);
                }
                _ => break,
            }
        }
        hardforks
    }

    pub(super) fn validate_hardfork_sequence(
        hardforks: &HashMap<Hardfork, u32>,
    ) -> Result<(), ProtocolConfigError> {
        let all = HardforkManager::all();
        let mut previous_index: Option<usize> = None;
        let mut previous_height: Option<u32> = None;

        for (index, hardfork) in all.iter().enumerate() {
            if let Some(&height) = hardforks.get(hardfork) {
                if let Some(prev_index) = previous_index {
                    if index - prev_index > 1 {
                        let missing = all[prev_index + 1];
                        return Err(ProtocolConfigError::InvalidHardforkSequence(format!(
                            "Hardfork {:?} is configured while {:?} is missing. Configure every hardfork sequentially.",
                            hardfork, missing
                        )));
                    }
                }

                if let Some(prev_height) = previous_height {
                    if height < prev_height {
                        return Err(ProtocolConfigError::InvalidHardforkSequence(format!(
                            "Hardfork {:?} activates at block {}, which is before previously configured height {}.",
                            hardfork, height, prev_height
                        )));
                    }
                }

                previous_index = Some(index);
                previous_height = Some(height);
            }
        }

        Ok(())
    }
}
