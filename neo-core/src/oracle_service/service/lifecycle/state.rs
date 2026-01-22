use super::super::{OracleService, OracleServiceError, OracleServiceSettings, OracleStatus};
#[cfg(feature = "oracle")]
use super::super::{OracleHttpsProtocol, OracleNeoFsProtocol};
use crate::neo_system::NeoSystem;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
#[cfg(feature = "oracle")]
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Weak};

impl OracleService {
    pub fn new(
        settings: OracleServiceSettings,
        system: Arc<NeoSystem>,
    ) -> Result<Self, OracleServiceError> {
        let mut settings = settings;
        settings.normalize();
        Ok(Self {
            settings,
            system,
            status: AtomicU8::new(OracleStatus::Unstarted.as_u8()),
            self_ref: RwLock::new(Weak::new()),
            wallet: RwLock::new(None),
            pending_queue: Mutex::new(HashMap::new()),
            finished_cache: Mutex::new(HashMap::new()),
            cancel: AtomicBool::new(false),
            request_task: Mutex::new(None),
            timer_task: Mutex::new(None),
            #[cfg(feature = "oracle")]
            counter: AtomicU64::new(1),
            #[cfg(feature = "oracle")]
            https: OracleHttpsProtocol::new(),
            #[cfg(feature = "oracle")]
            neofs: OracleNeoFsProtocol::new(),
        })
    }

    pub fn set_self_ref(self: &Arc<Self>) {
        *self.self_ref.write() = Arc::downgrade(self);
    }

    pub fn settings(&self) -> &OracleServiceSettings {
        &self.settings
    }

    pub fn status(&self) -> OracleStatus {
        OracleStatus::from_u8(self.status.load(Ordering::SeqCst))
    }

    pub fn is_running(&self) -> bool {
        self.status() == OracleStatus::Running
    }
}
