//! TokensTracker runtime.
//!
//! Provides balance/transfer indexing for NEP-17/NEP-11 standards. This struct is
//! registered as a committing/committed handler to process block events.

use super::settings::TokensTrackerSettings;
use super::trackers::nep_11::Nep11Tracker;
use super::trackers::nep_17::Nep17Tracker;
use super::trackers::tracker_base::Tracker;
use crate::i_event_handlers::{ICommittedHandler, ICommittingHandler};
use crate::neo_ledger::{ApplicationExecuted, Block};
use crate::persistence::{DataCache, IStore};
use crate::NeoSystem;
use parking_lot::RwLock;
use std::sync::Arc;

/// Runtime handler for token balance/transfer tracking.
///
/// Implements `ICommittingHandler` and `ICommittedHandler` to index
/// token transfers during block commits.
pub struct TokensTracker {
    settings: TokensTrackerSettings,
    trackers: RwLock<Vec<Box<dyn Tracker>>>,
}

impl TokensTracker {
    /// Creates a new TokensTracker with the given configuration.
    ///
    /// # Arguments
    ///
    /// * `settings` - Tracker configuration
    /// * `db` - Database store for balance/transfer data
    /// * `neo_system` - Reference to the Neo system
    pub fn new(
        settings: TokensTrackerSettings,
        db: Arc<dyn IStore>,
        neo_system: Arc<NeoSystem>,
    ) -> Self {
        let mut trackers: Vec<Box<dyn Tracker>> = Vec::new();

        if settings.enabled_nep17() {
            trackers.push(Box::new(Nep17Tracker::new(
                Arc::clone(&db),
                settings.max_results,
                settings.track_history,
                Arc::clone(&neo_system),
            )));
        }

        if settings.enabled_nep11() {
            trackers.push(Box::new(Nep11Tracker::new(
                Arc::clone(&db),
                settings.max_results,
                settings.track_history,
                Arc::clone(&neo_system),
            )));
        }

        Self {
            settings,
            trackers: RwLock::new(trackers),
        }
    }

    /// Returns a reference to the settings.
    pub fn settings(&self) -> &TokensTrackerSettings {
        &self.settings
    }
}

impl ICommittingHandler for TokensTracker {
    fn blockchain_committing_handler(
        &self,
        system: &NeoSystem,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    ) {
        if system.settings().network != self.settings.network {
            return;
        }

        let mut trackers = self.trackers.write();
        for tracker in trackers.iter_mut() {
            tracker.reset_batch();
            tracker.on_persist(system, block, snapshot, application_executed_list);
        }
    }
}

impl ICommittedHandler for TokensTracker {
    fn blockchain_committed_handler(&self, system: &NeoSystem, _block: &Block) {
        if system.settings().network != self.settings.network {
            return;
        }

        let mut trackers = self.trackers.write();
        for tracker in trackers.iter_mut() {
            tracker.commit();
        }
    }
}
