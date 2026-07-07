//! Invoke-session storage and expiration helpers.
//!
//! Sessions contain VM iterator state and must be protected by exclusive access
//! because the underlying execution engine is not thread-safe.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use uuid::Uuid;

use crate::server::session::Session;

use super::RpcServer;

pub(super) type SessionStore = Arc<Mutex<HashMap<Uuid, Session>>>;

pub(super) fn new_session_store() -> SessionStore {
    Arc::new(Mutex::new(HashMap::new()))
}

impl RpcServer {
    const fn session_expiration(&self) -> Duration {
        Duration::from_secs(self.settings.session_expiration_time)
    }

    /// Return whether invoke sessions are enabled.
    #[must_use]
    pub const fn session_enabled(&self) -> bool {
        self.settings.session_enabled
    }

    /// Remove expired RPC invoke sessions.
    pub fn purge_expired_sessions(&self) {
        if !self.session_enabled() {
            return;
        }
        let expiration = self.session_expiration();
        let mut guard = self.sessions.lock();
        guard.retain(|_, session| !session.is_expired(expiration));
    }

    /// Store an invoke session and return its generated id.
    pub fn store_session(&self, session: Session) -> Uuid {
        let id = Uuid::new_v4();
        self.sessions.lock().insert(id, session);
        id
    }

    /// Mutably access a stored session by id.
    pub fn with_session_mut<F, R>(&self, id: &Uuid, func: F) -> Option<R>
    where
        F: FnOnce(&mut Session) -> R,
    {
        let mut guard = self.sessions.lock();
        guard.get_mut(id).map(func)
    }

    /// Remove a stored session by id.
    #[must_use]
    pub fn terminate_session(&self, id: &Uuid) -> bool {
        self.sessions.lock().remove(id).is_some()
    }
}
