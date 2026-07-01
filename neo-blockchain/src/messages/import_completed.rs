//! Notification that block import has completed.

use serde::{Deserialize, Serialize};

/// Notification that block import has completed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCompleted;
