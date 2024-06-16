// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::ops::DerefMut;
use std::sync::{mpsc, Mutex};
use crate::dbft_v2::{HView, ViewNumber};


#[derive(Debug, Copy, Clone)]
pub struct Timer {
    pub view: HView,
    pub start_unix_milli: u64,
    pub delay_millis: u64,
}

impl Timer {
    #[inline]
    pub fn remain_millis(&self, now: u64) -> i64 {
        self.delay_millis as i64 - (now - self.start_unix_milli) as i64
    }
}

#[allow(dead_code)]
struct GuardTimer {
    timer: Timer,
    guard: Option<timer::Guard>,
}

pub struct ViewTimer {
    driver: timer::Timer,
    unix_milli_now: fn() -> u64,
    tx: mpsc::SyncSender<Timer>,
    timer: Mutex<GuardTimer>,
}

impl ViewTimer {
    pub fn new(unix_milli_now: fn() -> u64, tx: mpsc::SyncSender<Timer>) -> Self {
        let timer = Timer {
            view: HView::default(),
            start_unix_milli: unix_milli_now(),
            delay_millis: 0,
        };
        Self {
            driver: timer::Timer::new(),
            unix_milli_now,
            tx,
            timer: Mutex::new(GuardTimer { timer, guard: None }),
        }
    }

    pub fn extend_timeout(&self, view: HView, extend_millis: u64) {
        let now = (self.unix_milli_now)();
        let mut guards = self.timer.lock().unwrap();

        let delay_millis = guards.timer.remain_millis(now) + extend_millis as i64;
        if delay_millis > 0 {
            *(guards.deref_mut()) = GuardTimer {
                timer: Timer { view, start_unix_milli: now, delay_millis: delay_millis as u64 },
                guard: Some(self.schedule(view, delay_millis, now)),
            };
        }
    }

    pub fn reset_timeout(&self, view: HView, delay_millis: u64) {
        let now = (self.unix_milli_now)();
        let mut guards = self.timer.lock().unwrap();
        *(guards.deref_mut()) = GuardTimer {
            timer: Timer { view, start_unix_milli: now, delay_millis },
            guard: Some(self.schedule(view, delay_millis as i64, now)),
        };
    }

    fn schedule(&self, view: HView, delay_millis: i64, now: u64) -> timer::Guard {
        let delay = chrono::Duration::milliseconds(delay_millis);
        let timer = Timer { view, start_unix_milli: now, delay_millis: delay_millis as u64 };

        let send = timer.clone();
        let tx = self.tx.clone();
        self.driver.schedule_with_delay(delay, move || {
            tx.send(send).expect("`send` should be ok");
        })
    }
}


#[inline]
pub fn millis_on_setting(view_number: ViewNumber, millis_per_block: u64) -> u64 {
    millis_per_block << core::cmp::min(32, view_number + 1)
}

#[inline]
pub fn millis_on_resetting(primary: bool, view_number: ViewNumber, millis_per_block: u64) -> u64 {
    if primary {
        if view_number == 0 { millis_per_block } else { 0 }
    } else {
        millis_per_block << core::cmp::min(32, view_number + 1)
    }
}


#[inline]
pub fn millis_on_timeout(view_number: ViewNumber, millis_per_block: u64) -> u64 {
    if view_number == 0 {
        millis_per_block
    } else {
        millis_per_block << core::cmp::min(32, view_number + 1)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::unix_milli_now;

    #[test]
    fn test_timer() {
        let (tx, rx) = mpsc::sync_channel(1);
        let vt = ViewTimer::new(unix_milli_now, tx);
        vt.reset_timeout(HView { height: 2, view_number: 1 }, 100);

        let timer = rx.recv().expect("`recv` should be ok");
        assert_eq!(timer.view, HView { height: 2, view_number: 1 });
        assert_eq!(timer.delay_millis, 100);

        vt.extend_timeout(HView { height: 2, view_number: 2 }, 200);
        let timer = rx.recv().expect("`recv` should be ok");
        assert_eq!(timer.view, HView { height: 2, view_number: 2 });
    }
}
