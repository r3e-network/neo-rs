//! Notification that memory pool fill has completed.

use serde::{Deserialize, Serialize};

/// Notification that memory pool fill has completed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillCompleted;
