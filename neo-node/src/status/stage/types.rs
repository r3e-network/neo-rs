use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StageState {
    Inactive,
    Pending,
    Complete,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct StageStatus {
    pub state: StageState,
    pub expected: Option<usize>,
    pub responded: usize,
    pub missing: Vec<u16>,
    pub last_updated: DateTime<Utc>,
    pub age_ms: u128,
    pub stale: bool,
}
