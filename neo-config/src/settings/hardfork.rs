// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! Deterministic Neo N3 hardfork schedules.
//!
//! Neo v3.10.1 defines exactly eight height-activated hardforks. A fixed table
//! is cheaper and more deterministic than a hash map, while still preserving
//! the C# rule that configured forks form a monotonic, gap-free prefix.

use thiserror::Error;

pub use neo_primitives::{Hardfork, HardforkParseError};

/// A compact set of configured or active Neo hardforks.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ActiveHardforks(u8);

impl ActiveHardforks {
    /// Returns an empty set.
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Returns whether `hardfork` belongs to this set.
    #[must_use]
    pub const fn contains(self, hardfork: Hardfork) -> bool {
        self.0 & (1 << hardfork.index()) != 0
    }

    /// Returns the stable bit representation, indexed by [`Hardfork::index`].
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Iterates contained hardforks in declaration order.
    pub fn iter(self) -> impl Iterator<Item = Hardfork> {
        Hardfork::ALL
            .into_iter()
            .filter(move |hardfork| self.contains(*hardfork))
    }

    const fn with(mut self, hardfork: Hardfork) -> Self {
        self.0 |= 1 << hardfork.index();
        self
    }
}

/// Validation failure for a Neo hardfork schedule.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum HardforkScheduleError {
    /// A later hardfork was configured after an omitted predecessor.
    #[error("{configured:?} is configured while predecessor {missing:?} is missing")]
    Gap {
        /// The configured hardfork after the gap.
        configured: Hardfork,
        /// The first missing predecessor.
        missing: Hardfork,
    },

    /// A later hardfork activates below the previous activation height.
    #[error(
        "{hardfork:?} activates at block {height}, below previous activation height {previous_height}"
    )]
    NonMonotonic {
        /// The invalid hardfork.
        hardfork: Hardfork,
        /// Its configured activation height.
        height: u32,
        /// The preceding configured height.
        previous_height: u32,
    },
}

/// Fixed, declaration-ordered activation heights for Neo N3 hardforks.
///
/// The table has no heap allocation, hashing, or nondeterministic iteration.
/// `None` means the fork is not configured for this chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct HardforkSchedule {
    heights: [Option<u32>; Hardfork::COUNT],
}

impl HardforkSchedule {
    /// Returns a schedule with no configured hardforks.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            heights: [None; Hardfork::COUNT],
        }
    }

    /// Returns the C# v3.10.1 default schedule, with every fork active at genesis.
    #[must_use]
    pub const fn csharp_default() -> Self {
        Self {
            heights: [Some(0); Hardfork::COUNT],
        }
    }

    /// Returns the Neo N3 v3.10.1 MainNet schedule.
    #[must_use]
    pub const fn mainnet() -> Self {
        Self::new()
            .with_activation(Hardfork::HfAspidochelone, 1_730_000)
            .with_activation(Hardfork::HfBasilisk, 4_120_000)
            .with_activation(Hardfork::HfCockatrice, 5_450_000)
            .with_activation(Hardfork::HfDomovoi, 5_570_000)
            .with_activation(Hardfork::HfEchidna, 7_300_000)
            .with_activation(Hardfork::HfFaun, 8_800_000)
            .with_activation(Hardfork::HfGorgon, 12_020_000)
    }

    /// Returns the Neo N3 v3.10.1 TestNet schedule.
    #[must_use]
    pub const fn testnet() -> Self {
        Self::new()
            .with_activation(Hardfork::HfAspidochelone, 210_000)
            .with_activation(Hardfork::HfBasilisk, 2_680_000)
            .with_activation(Hardfork::HfCockatrice, 3_967_000)
            .with_activation(Hardfork::HfDomovoi, 4_144_000)
            .with_activation(Hardfork::HfEchidna, 5_870_000)
            .with_activation(Hardfork::HfFaun, 12_960_000)
            .with_activation(Hardfork::HfGorgon, 17_960_000)
    }

    /// Returns a new schedule with one activation installed.
    #[must_use]
    pub const fn with_activation(mut self, hardfork: Hardfork, height: u32) -> Self {
        self.heights[hardfork.index() as usize] = Some(height);
        self
    }

    /// Returns a new schedule without `hardfork`.
    #[must_use]
    pub const fn without_activation(mut self, hardfork: Hardfork) -> Self {
        self.heights[hardfork.index() as usize] = None;
        self
    }

    /// Returns a new schedule with the supplied activations installed.
    #[must_use]
    pub fn with_activations<I>(mut self, activations: I) -> Self
    where
        I: IntoIterator<Item = (Hardfork, u32)>,
    {
        for (hardfork, height) in activations {
            self = self.with_activation(hardfork, height);
        }
        self
    }

    /// Applies a deterministic transformation to every configured activation.
    #[must_use]
    pub fn map_activation_heights<F>(mut self, mut transform: F) -> Self
    where
        F: FnMut(Hardfork, u32) -> u32,
    {
        for hardfork in Hardfork::ALL {
            if let Some(height) = self.activation_height(hardfork) {
                self = self.with_activation(hardfork, transform(hardfork, height));
            }
        }
        self
    }

    /// Applies Neo's C# omitted-leading-hardfork rule.
    ///
    /// Missing forks before the first configured fork activate at genesis. An
    /// entirely empty configuration therefore enables every known fork at zero.
    #[must_use]
    pub fn with_omitted_leading_at_genesis(mut self) -> Self {
        let first_configured = self.heights.iter().position(Option::is_some);
        let fill_through = first_configured.unwrap_or(self.heights.len());
        for height in &mut self.heights[..fill_through] {
            *height = Some(0);
        }
        self
    }

    /// Validates the gap-free, non-decreasing C# schedule contract.
    pub fn validate(&self) -> Result<(), HardforkScheduleError> {
        let mut previous_height = None;
        let mut first_missing = None;

        for hardfork in Hardfork::ALL {
            match self.activation_height(hardfork) {
                Some(height) => {
                    if let Some(missing) = first_missing {
                        return Err(HardforkScheduleError::Gap {
                            configured: hardfork,
                            missing,
                        });
                    }
                    if let Some(previous_height) = previous_height
                        && height < previous_height
                    {
                        return Err(HardforkScheduleError::NonMonotonic {
                            hardfork,
                            height,
                            previous_height,
                        });
                    }
                    previous_height = Some(height);
                }
                None => {
                    first_missing.get_or_insert(hardfork);
                }
            }
        }

        Ok(())
    }

    /// Returns the activation height for `hardfork`.
    #[must_use]
    pub const fn activation_height(&self, hardfork: Hardfork) -> Option<u32> {
        self.heights[hardfork.index() as usize]
    }

    /// Returns whether `hardfork` is configured.
    #[must_use]
    pub const fn is_defined(&self, hardfork: Hardfork) -> bool {
        self.activation_height(hardfork).is_some()
    }

    /// Returns whether `hardfork` is active at `block_height`.
    #[must_use]
    pub const fn is_active(&self, hardfork: Hardfork, block_height: u32) -> bool {
        match self.activation_height(hardfork) {
            Some(height) => block_height >= height,
            None => false,
        }
    }

    /// Returns every configured fork as a compact bit set.
    #[must_use]
    pub fn defined_set(&self) -> ActiveHardforks {
        Hardfork::ALL
            .into_iter()
            .filter(|hardfork| self.is_defined(*hardfork))
            .fold(ActiveHardforks::empty(), ActiveHardforks::with)
    }

    /// Returns the forks active at `block_height` as a compact bit set.
    #[must_use]
    pub fn active_at(&self, block_height: u32) -> ActiveHardforks {
        Hardfork::ALL
            .into_iter()
            .filter(|hardfork| self.is_active(*hardfork, block_height))
            .fold(ActiveHardforks::empty(), ActiveHardforks::with)
    }

    /// Returns the number of configured hardforks.
    #[must_use]
    pub fn len(&self) -> usize {
        self.heights
            .iter()
            .filter(|height| height.is_some())
            .count()
    }

    /// Returns whether no hardfork is configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Iterates configured activations in hardfork declaration order.
    pub fn iter(&self) -> impl Iterator<Item = (Hardfork, u32)> + '_ {
        Hardfork::ALL
            .into_iter()
            .zip(self.heights.iter())
            .filter_map(|(hardfork, height)| height.map(|height| (hardfork, height)))
    }

    /// Iterates configured activation heights in declaration order.
    pub fn activation_heights(&self) -> impl Iterator<Item = u32> + '_ {
        self.heights.iter().filter_map(|height| *height)
    }
}

impl Default for HardforkSchedule {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[path = "../tests/settings/hardfork.rs"]
mod tests;
