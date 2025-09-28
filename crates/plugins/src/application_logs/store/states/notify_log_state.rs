//! Notify Log State
//!
//! State management for notification logs.

use serde::{Deserialize, Serialize};

/// Notify Log State
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotifyLogState {
    /// Event name
    pub event_name: String,
    /// Contract hash
    pub contract_hash: String,
    /// Notification data
    pub notification_data: String,
}
