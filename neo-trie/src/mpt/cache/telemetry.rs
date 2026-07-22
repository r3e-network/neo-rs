use std::fs;
use std::time::Instant;

use super::MptMutationStats;

pub(super) fn elapsed_us(start: Instant) -> u64 {
    start.elapsed().as_micros().min(u64::MAX as u128) as u64
}

pub(super) fn elapsed_ns(start: Instant) -> u64 {
    start.elapsed().as_nanos().min(u64::MAX as u128) as u64
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProcessResourceSnapshot {
    pub(crate) read_bytes: u64,
    pub(crate) minor_faults: u64,
    pub(crate) major_faults: u64,
}

impl ProcessResourceSnapshot {
    fn delta_since(self, before: Self) -> Self {
        Self {
            read_bytes: self.read_bytes.saturating_sub(before.read_bytes),
            minor_faults: self.minor_faults.saturating_sub(before.minor_faults),
            major_faults: self.major_faults.saturating_sub(before.major_faults),
        }
    }
}

impl MptMutationStats {
    pub(crate) fn record_deferred_resource_delta(
        &mut self,
        before: Option<ProcessResourceSnapshot>,
        after: Option<ProcessResourceSnapshot>,
    ) {
        let (Some(before), Some(after)) = (before, after) else {
            return;
        };
        let delta = after.delta_since(before);
        self.deferred_finalization_read_bytes = self
            .deferred_finalization_read_bytes
            .saturating_add(delta.read_bytes);
        self.deferred_finalization_minor_faults = self
            .deferred_finalization_minor_faults
            .saturating_add(delta.minor_faults);
        self.deferred_finalization_major_faults = self
            .deferred_finalization_major_faults
            .saturating_add(delta.major_faults);
    }
}

/// Best-effort Linux process resource counters. Other platforms and restricted
/// containers return `None`; callers must treat that as missing telemetry, not
/// as a storage or execution failure.
pub(super) fn process_resource_snapshot() -> Option<ProcessResourceSnapshot> {
    let io = fs::read_to_string("/proc/self/io").ok()?;
    let stat = fs::read_to_string("/proc/self/stat").ok()?;
    let (minor_faults, major_faults) = proc_stat_faults(&stat)?;
    Some(ProcessResourceSnapshot {
        read_bytes: proc_io_counter(&io, "read_bytes")?,
        minor_faults,
        major_faults,
    })
}

pub(crate) fn proc_io_counter(input: &str, name: &str) -> Option<u64> {
    input.lines().find_map(|line| {
        let (field, value) = line.split_once(':')?;
        (field == name).then(|| value.trim().parse().ok()).flatten()
    })
}

pub(crate) fn proc_stat_faults(input: &str) -> Option<(u64, u64)> {
    let mut fields = input.get(input.rfind(')')? + 1..)?.split_whitespace();
    // After the parenthesized process name, state is field 3 (index 0).
    let minor_faults = fields.nth(7)?.parse().ok()?;
    let major_faults = fields.nth(1)?.parse().ok()?;
    Some((minor_faults, major_faults))
}
