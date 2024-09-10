// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use std::ops::{Add, Sub};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed};
use std::time::Duration;

use chrono::TimeDelta;

pub type LocalTime = chrono::DateTime<chrono::Local>;

pub type UtcTime = chrono::DateTime<chrono::Utc>;

#[derive(Debug, Default, Copy, Clone, Eq, PartialEq)]
pub struct UnixTime(i64);

impl UnixTime {
    #[inline]
    pub fn now() -> UnixTime {
        Self(utc_now().timestamp_micros())
    }

    pub const fn from_millis(millis: i64) -> Self {
        Self(millis)
    }

    pub const fn unix_micros(&self) -> i64 {
        self.0
    }

    pub const fn unix_millis(&self) -> i64 {
        self.0 / 1000
    }

    pub const fn unix_seconds(&self) -> i64 {
        self.0 / 1000 / 1000
    }

    #[inline]
    pub fn since(u: UnixTime) -> TimeDelta {
        TimeDelta::microseconds(Self::now().0 - u.0)
    }
}

impl Sub for UnixTime {
    type Output = TimeDelta;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        TimeDelta::microseconds(self.0 - rhs.0)
    }
}

impl Add<Duration> for UnixTime {
    type Output = UnixTime;

    #[inline]
    fn add(self, rhs: Duration) -> Self::Output {
        Self(self.unix_micros() + rhs.as_micros() as i64)
    }
}

impl Sub<Duration> for UnixTime {
    type Output = UnixTime;

    #[inline]
    fn sub(self, rhs: Duration) -> Self::Output {
        Self(self.unix_micros() - rhs.as_micros() as i64)
    }
}

impl Add<TimeDelta> for UnixTime {
    type Output = UnixTime;

    #[inline]
    fn add(self, rhs: TimeDelta) -> Self::Output {
        Self(
            self.unix_micros()
                + rhs
                    .num_microseconds()
                    .unwrap_or_else(|| rhs.num_milliseconds() * 1000),
        )
    }
}

impl Sub<TimeDelta> for UnixTime {
    type Output = UnixTime;

    #[inline]
    fn sub(self, rhs: TimeDelta) -> Self::Output {
        Self(
            self.unix_micros()
                - rhs
                    .num_microseconds()
                    .unwrap_or_else(|| rhs.num_milliseconds() * 1000),
        )
    }
}

#[derive(Debug, Default)]
pub struct AtomicUnixTime(AtomicI64);

impl AtomicUnixTime {
    #[inline]
    pub fn now() -> AtomicUnixTime {
        Self(AtomicI64::new(utc_now().timestamp_micros()))
    }

    pub const fn from_millis(millis: i64) -> Self {
        Self(AtomicI64::new(millis))
    }

    pub fn unix_micros(&self) -> i64 {
        self.0.load(Relaxed)
    }

    pub fn unix_millis(&self) -> i64 {
        self.unix_micros() / 1000
    }

    pub fn unix_seconds(&self) -> i64 {
        self.unix_micros() / 1000 / 1000
    }

    pub fn load(&self) -> UnixTime {
        UnixTime(self.unix_micros())
    }

    pub fn store(&self, new: UnixTime) {
        self.0.store(new.0, Relaxed)
    }
}

#[inline]
pub fn unix_millis_now() -> u64 {
    UnixTime::now().unix_millis() as u64
}

#[inline]
pub fn unix_seconds_now() -> u64 {
    UnixTime::now().unix_seconds() as u64
}

#[inline]
pub fn utc_now() -> UtcTime {
    chrono::Utc::now()
}

#[inline]
pub fn local_now() -> LocalTime {
    chrono::Local::now()
}

pub struct Tick {
    unix_millis: AtomicU64,
    interval: Duration,
    check_millis: u64,
    stopped: AtomicBool,
}

impl Tick {
    pub fn new(interval: Duration) -> Self {
        Self {
            unix_millis: AtomicU64::new(unix_millis_now()),
            interval,
            check_millis: 500,
            stopped: AtomicBool::new(false),
        }
    }

    pub fn wait(&self) -> bool {
        let next = self.unix_millis.load(Relaxed) + self.interval.as_millis() as u64;
        let mut now = unix_millis_now();
        while !self.stopped.load(Relaxed) && now < next {
            let millis = core::cmp::min(self.check_millis, next - now);
            std::thread::sleep(Duration::from_millis(millis));

            now = unix_millis_now();
        }

        self.unix_millis.store(now, Relaxed);
        !self.is_stopped()
    }

    #[inline]
    pub fn is_stopped(&self) -> bool {
        self.stopped.load(Relaxed)
    }

    #[inline]
    pub fn stop(&self) {
        self.stopped.store(true, Relaxed)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_tick() {
        let now = UnixTime::now();
        let iv = Tick::new(Duration::from_millis(200));
        iv.wait();

        let delta = UnixTime::since(now);
        assert!(delta.num_milliseconds() >= 200);
    }

    #[test]
    fn test_local_now() {
        assert_eq!(unix_seconds_now(), local_now().timestamp() as u64);
    }
}
