//! Oracle service lifecycle status.

/// Runtime state of the oracle service.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OracleStatus {
    /// Service has been constructed but never started.
    Unstarted,
    /// Service is actively polling and processing oracle requests.
    Running,
    /// Service has been stopped and background tasks have been cancelled.
    Stopped,
}

impl OracleStatus {
    pub(super) fn as_u8(self) -> u8 {
        match self {
            OracleStatus::Unstarted => 0,
            OracleStatus::Running => 1,
            OracleStatus::Stopped => 2,
        }
    }

    pub(super) fn from_u8(value: u8) -> Self {
        match value {
            1 => OracleStatus::Running,
            2 => OracleStatus::Stopped,
            _ => OracleStatus::Unstarted,
        }
    }
}
