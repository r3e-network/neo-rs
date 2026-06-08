use super::super::{
    ExpiringSet, FINISHED_CACHE_TTL, OracleDedupState, OracleService, OracleServiceError,
    OracleServiceSettings, OracleStatus,
};
#[cfg(feature = "oracle")]
use super::super::{OracleHttpsProtocol, OracleNeoFsProtocol};
use neo_system::Node;
use parking_lot::{Mutex, RwLock};
use std::collections::HashMap;
#[cfg(feature = "oracle")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Weak};

impl OracleService {
    pub fn new(
        settings: OracleServiceSettings,
        system: Arc<Node>,
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
            finished_cache: Mutex::new(ExpiringSet::new(FINISHED_CACHE_TTL)),
            dedup: Mutex::new(OracleDedupState::default()),
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
