// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

pub type LocalTime = chrono::DateTime<chrono::Local>;

pub type UtcTime = chrono::DateTime<chrono::Utc>;

#[inline]
pub fn utc_now() -> UtcTime {
    chrono::Utc::now()
}

#[inline]
pub fn local_now() -> LocalTime {
    chrono::Local::now()
}

#[inline]
pub fn unix_millis_now() -> u64 {
    utc_now().timestamp_millis() as u64
}

#[inline]
pub fn unix_seconds_now() -> u64 {
    utc_now().timestamp() as u64
}
