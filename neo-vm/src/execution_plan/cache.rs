//! Bounded concurrent single-flight cache for immutable execution plans.

use super::{ExecutionPlan, ExecutionPlanBuildError, ExecutionPlanKey, ExecutionPlanLimits};
use parking_lot::{Condvar, Mutex};
use std::collections::HashMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

/// Hard entry and byte limits for an [`ExecutionPlanCache`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionPlanCacheLimits {
    /// Maximum ready and in-flight key count.
    pub max_entries: usize,
    /// Maximum ready-plan bytes plus in-flight exact-script reservations.
    pub max_bytes: usize,
}

impl ExecutionPlanCacheLimits {
    /// Conservative opt-in defaults for a small hot-contract population.
    pub const DEFAULT: Self = Self {
        max_entries: 256,
        max_bytes: 64 * 1024 * 1024,
    };
}

impl Default for ExecutionPlanCacheLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// A cache-level failure. Ordinary `neo-vm` execution remains the fallback.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionPlanCacheError {
    /// Plan validation or construction rejected the script.
    Build(ExecutionPlanBuildError),
    /// The configured cache cannot reserve the exact key bytes or an entry.
    Capacity,
    /// Plan construction panicked before a plan became visible.
    ConstructionPanicked,
}

impl std::fmt::Display for ExecutionPlanCacheError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Build(error) => write!(formatter, "execution plan build failed: {error}"),
            Self::Capacity => formatter.write_str("execution plan cache capacity exhausted"),
            Self::ConstructionPanicked => {
                formatter.write_str("execution plan construction panicked")
            }
        }
    }
}

impl std::error::Error for ExecutionPlanCacheError {}

impl From<ExecutionPlanBuildError> for ExecutionPlanCacheError {
    fn from(error: ExecutionPlanBuildError) -> Self {
        Self::Build(error)
    }
}

/// Point-in-time bounded cache counters.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ExecutionPlanCacheSnapshot {
    /// Ready cached plans.
    pub ready_entries: usize,
    /// Currently constructing keys.
    pub in_flight_entries: usize,
    /// Accounted ready plan bytes.
    pub ready_bytes: usize,
    /// Exact script bytes reserved by in-flight builders.
    pub reserved_bytes: usize,
    /// Successful ready-plan lookups.
    pub hits: u64,
    /// First callers that did not find a key.
    pub misses: u64,
    /// Callers that waited for an in-flight builder.
    pub waits: u64,
    /// Single-flight plan construction attempts.
    pub builds: u64,
    /// Ready plans removed for capacity.
    pub evictions: u64,
    /// Build or panic failures published to callers.
    pub build_failures: u64,
    /// Requests rejected by entry or byte capacity.
    pub capacity_rejections: u64,
}

#[derive(Debug)]
struct BuildSlot {
    result: Mutex<Option<Result<Arc<ExecutionPlan>, ExecutionPlanCacheError>>>,
    ready: Condvar,
}

impl BuildSlot {
    fn new() -> Self {
        Self {
            result: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn publish(&self, result: Result<Arc<ExecutionPlan>, ExecutionPlanCacheError>) {
        *self.result.lock() = Some(result);
        self.ready.notify_all();
    }

    fn wait(&self) -> Result<Arc<ExecutionPlan>, ExecutionPlanCacheError> {
        let mut result = self.result.lock();
        while result.is_none() {
            self.ready.wait(&mut result);
        }
        result.as_ref().expect("result checked above").clone()
    }
}

#[derive(Debug)]
enum CacheEntry {
    Building {
        slot: Arc<BuildSlot>,
        reserved_bytes: usize,
    },
    Ready {
        plan: Arc<ExecutionPlan>,
        last_used: u64,
    },
}

#[derive(Debug, Default)]
struct CacheState {
    entries: HashMap<ExecutionPlanKey, CacheEntry>,
    ready_bytes: usize,
    reserved_bytes: usize,
    clock: u64,
    hits: u64,
    misses: u64,
    waits: u64,
    builds: u64,
    evictions: u64,
    build_failures: u64,
    capacity_rejections: u64,
}

impl CacheState {
    fn tick(&mut self) -> u64 {
        self.clock = self.clock.saturating_add(1);
        self.clock
    }
}

enum LookupAction {
    Return(Arc<ExecutionPlan>),
    Wait(Arc<BuildSlot>),
    Build(Arc<BuildSlot>),
    Reject,
}

/// Concurrent exact-key cache whose misses are single-flight per key.
///
/// The cache owns no execution output. A dropped or evicted plan is always
/// rebuildable, and all capacity failures select ordinary VM execution.
#[derive(Debug)]
pub struct ExecutionPlanCache {
    limits: ExecutionPlanCacheLimits,
    plan_limits: ExecutionPlanLimits,
    state: Mutex<CacheState>,
}

impl ExecutionPlanCache {
    /// Creates an empty cache with explicit plan-construction and residency bounds.
    #[must_use]
    pub fn new(limits: ExecutionPlanCacheLimits, plan_limits: ExecutionPlanLimits) -> Self {
        Self {
            limits,
            plan_limits,
            state: Mutex::new(CacheState::default()),
        }
    }

    /// Returns or constructs the exact immutable plan for `key`.
    ///
    /// Construction occurs without holding the cache lock. Concurrent callers
    /// for the same key wait on one bounded slot and receive the same success or
    /// failure. Panics are contained before any plan is published.
    pub fn get_or_build(
        &self,
        key: ExecutionPlanKey,
    ) -> Result<Arc<ExecutionPlan>, ExecutionPlanCacheError> {
        let action = self.lookup_or_reserve(&key);
        match action {
            LookupAction::Return(plan) => Ok(plan),
            LookupAction::Wait(slot) => slot.wait(),
            LookupAction::Reject => Err(ExecutionPlanCacheError::Capacity),
            LookupAction::Build(slot) => {
                let built = catch_unwind(AssertUnwindSafe(|| {
                    ExecutionPlan::build(key.clone(), self.plan_limits)
                        .map(Arc::new)
                        .map_err(ExecutionPlanCacheError::from)
                }))
                .unwrap_or(Err(ExecutionPlanCacheError::ConstructionPanicked));
                let result = self.finish_build(&key, &slot, built);
                slot.publish(result.clone());
                result
            }
        }
    }

    /// Returns current size and race counters without exposing cache keys.
    #[must_use]
    pub fn snapshot(&self) -> ExecutionPlanCacheSnapshot {
        let state = self.state.lock();
        let ready_entries = state
            .entries
            .values()
            .filter(|entry| matches!(entry, CacheEntry::Ready { .. }))
            .count();
        let in_flight_entries = state.entries.len() - ready_entries;
        ExecutionPlanCacheSnapshot {
            ready_entries,
            in_flight_entries,
            ready_bytes: state.ready_bytes,
            reserved_bytes: state.reserved_bytes,
            hits: state.hits,
            misses: state.misses,
            waits: state.waits,
            builds: state.builds,
            evictions: state.evictions,
            build_failures: state.build_failures,
            capacity_rejections: state.capacity_rejections,
        }
    }

    fn lookup_or_reserve(&self, key: &ExecutionPlanKey) -> LookupAction {
        let mut state = self.state.lock();
        let tick = state.tick();
        if let Some(entry) = state.entries.get_mut(key) {
            return match entry {
                CacheEntry::Ready { plan, last_used } => {
                    *last_used = tick;
                    let plan = Arc::clone(plan);
                    state.hits = state.hits.saturating_add(1);
                    LookupAction::Return(plan)
                }
                CacheEntry::Building { slot, .. } => {
                    let slot = Arc::clone(slot);
                    state.waits = state.waits.saturating_add(1);
                    LookupAction::Wait(slot)
                }
            };
        }

        state.misses = state.misses.saturating_add(1);
        let reservation = key.script_len();
        if self.limits.max_entries == 0
            || reservation > self.limits.max_bytes
            || !make_room(&mut state, self.limits, reservation, 1)
        {
            state.capacity_rejections = state.capacity_rejections.saturating_add(1);
            return LookupAction::Reject;
        }

        let slot = Arc::new(BuildSlot::new());
        state.reserved_bytes += reservation;
        state.builds = state.builds.saturating_add(1);
        state.entries.insert(
            key.clone(),
            CacheEntry::Building {
                slot: Arc::clone(&slot),
                reserved_bytes: reservation,
            },
        );
        LookupAction::Build(slot)
    }

    fn finish_build(
        &self,
        key: &ExecutionPlanKey,
        slot: &Arc<BuildSlot>,
        built: Result<Arc<ExecutionPlan>, ExecutionPlanCacheError>,
    ) -> Result<Arc<ExecutionPlan>, ExecutionPlanCacheError> {
        let mut state = self.state.lock();
        let Some(CacheEntry::Building {
            slot: current,
            reserved_bytes,
        }) = state.entries.remove(key)
        else {
            state.build_failures = state.build_failures.saturating_add(1);
            return Err(ExecutionPlanCacheError::Capacity);
        };
        debug_assert!(Arc::ptr_eq(&current, slot));
        state.reserved_bytes = state.reserved_bytes.saturating_sub(reserved_bytes);

        let plan = match built {
            Ok(plan) => plan,
            Err(error) => {
                state.build_failures = state.build_failures.saturating_add(1);
                return Err(error);
            }
        };
        let plan_bytes = plan.accounted_bytes();
        if plan_bytes > self.limits.max_bytes || !make_room(&mut state, self.limits, plan_bytes, 1)
        {
            state.capacity_rejections = state.capacity_rejections.saturating_add(1);
            return Err(ExecutionPlanCacheError::Capacity);
        }

        let tick = state.tick();
        state.ready_bytes += plan_bytes;
        state.entries.insert(
            key.clone(),
            CacheEntry::Ready {
                plan: Arc::clone(&plan),
                last_used: tick,
            },
        );
        Ok(plan)
    }
}

impl Default for ExecutionPlanCache {
    fn default() -> Self {
        Self::new(
            ExecutionPlanCacheLimits::default(),
            ExecutionPlanLimits::default(),
        )
    }
}

fn make_room(
    state: &mut CacheState,
    limits: ExecutionPlanCacheLimits,
    additional_bytes: usize,
    additional_entries: usize,
) -> bool {
    loop {
        let bytes_fit = state
            .ready_bytes
            .checked_add(state.reserved_bytes)
            .and_then(|bytes| bytes.checked_add(additional_bytes))
            .is_some_and(|bytes| bytes <= limits.max_bytes);
        let entries_fit = state
            .entries
            .len()
            .checked_add(additional_entries)
            .is_some_and(|entries| entries <= limits.max_entries);
        if bytes_fit && entries_fit {
            return true;
        }

        let victim = state
            .entries
            .iter()
            .filter_map(|(key, entry)| match entry {
                CacheEntry::Ready { last_used, .. } => Some((key.clone(), *last_used)),
                CacheEntry::Building { .. } => None,
            })
            .min_by_key(|(_, last_used)| *last_used)
            .map(|(key, _)| key);
        let Some(victim) = victim else {
            return false;
        };
        if let Some(CacheEntry::Ready { plan, .. }) = state.entries.remove(&victim) {
            state.ready_bytes = state.ready_bytes.saturating_sub(plan.accounted_bytes());
            state.evictions = state.evictions.saturating_add(1);
        }
    }
}

#[cfg(test)]
#[path = "../tests/execution_plan/cache.rs"]
mod tests;
