//! Prometheus metrics for supervised daemon tasks.

use std::sync::atomic::{AtomicU64, Ordering};

use super::supervision::TaskKind;

const TASKS: &[&str] = &[
    "blockchain_service",
    "p2p_service",
    "inventory_relay",
    "consensus_driver",
    "telemetry_metrics",
    "network_height_advertiser",
    "indexer_runtime",
    "observability_heartbeat",
    "unknown",
];
const OUTCOMES: &[&str] = &["exit", "error", "panic"];
const UNKNOWN_TASK_INDEX: usize = TASKS.len() - 1;
const OUTCOME_EXIT: usize = 0;
const OUTCOME_ERROR: usize = 1;
const OUTCOME_PANIC: usize = 2;

struct TaskMetric {
    spawned_essential: AtomicU64,
    spawned_normal: AtomicU64,
    events_essential: [AtomicU64; 3],
    events_normal: [AtomicU64; 3],
}

impl TaskMetric {
    const fn new() -> Self {
        Self {
            spawned_essential: AtomicU64::new(0),
            spawned_normal: AtomicU64::new(0),
            events_essential: [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)],
            events_normal: [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)],
        }
    }

    fn spawned(&self, kind: TaskKind) -> &AtomicU64 {
        match kind {
            TaskKind::Essential => &self.spawned_essential,
            TaskKind::Normal => &self.spawned_normal,
        }
    }

    fn events(&self, kind: TaskKind) -> &[AtomicU64; 3] {
        match kind {
            TaskKind::Essential => &self.events_essential,
            TaskKind::Normal => &self.events_normal,
        }
    }
}

static METRICS: [TaskMetric; TASKS.len()] = [
    TaskMetric::new(),
    TaskMetric::new(),
    TaskMetric::new(),
    TaskMetric::new(),
    TaskMetric::new(),
    TaskMetric::new(),
    TaskMetric::new(),
    TaskMetric::new(),
    TaskMetric::new(),
];

pub(super) fn record_spawn(task_name: &'static str, kind: TaskKind) {
    metric(task_name)
        .spawned(kind)
        .fetch_add(1, Ordering::Relaxed);
}

pub(super) fn record_exit(task_name: &'static str, kind: TaskKind) {
    record_event(task_name, kind, OUTCOME_EXIT);
}

pub(super) fn record_error(task_name: &'static str, kind: TaskKind) {
    record_event(task_name, kind, OUTCOME_ERROR);
}

pub(super) fn record_panic(task_name: &'static str, kind: TaskKind) {
    record_event(task_name, kind, OUTCOME_PANIC);
}

pub(in crate::node) fn render_prometheus() -> String {
    let mut out = String::from(
        "# HELP neo_node_daemon_task_spawned_total Total supervised daemon task spawns\n\
         # TYPE neo_node_daemon_task_spawned_total counter\n",
    );
    for (index, task) in TASKS.iter().enumerate() {
        let metrics = &METRICS[index];
        for kind in [TaskKind::Essential, TaskKind::Normal] {
            out.push_str(&format!(
                "neo_node_daemon_task_spawned_total{{task=\"{}\",kind=\"{}\"}} {}\n",
                task,
                kind.as_label(),
                metrics.spawned(kind).load(Ordering::Relaxed)
            ));
        }
    }

    out.push_str(
        "# HELP neo_node_daemon_task_events_total Total supervised daemon task terminal events\n\
         # TYPE neo_node_daemon_task_events_total counter\n",
    );
    for (index, task) in TASKS.iter().enumerate() {
        let metrics = &METRICS[index];
        for kind in [TaskKind::Essential, TaskKind::Normal] {
            let events = metrics.events(kind);
            for (outcome_index, outcome) in OUTCOMES.iter().enumerate() {
                out.push_str(&format!(
                    "neo_node_daemon_task_events_total{{task=\"{}\",kind=\"{}\",outcome=\"{}\"}} {}\n",
                    task,
                    kind.as_label(),
                    outcome,
                    events[outcome_index].load(Ordering::Relaxed)
                ));
            }
        }
    }
    out
}

#[cfg(test)]
pub(in crate::node) fn reset_for_tests() {
    for metrics in &METRICS {
        metrics.spawned_essential.store(0, Ordering::Relaxed);
        metrics.spawned_normal.store(0, Ordering::Relaxed);
        for event in &metrics.events_essential {
            event.store(0, Ordering::Relaxed);
        }
        for event in &metrics.events_normal {
            event.store(0, Ordering::Relaxed);
        }
    }
}

fn record_event(task_name: &'static str, kind: TaskKind, outcome: usize) {
    metric(task_name).events(kind)[outcome].fetch_add(1, Ordering::Relaxed);
}

fn metric(task_name: &str) -> &'static TaskMetric {
    let index = TASKS
        .iter()
        .position(|task| *task == task_name)
        .unwrap_or(UNKNOWN_TASK_INDEX);
    &METRICS[index]
}
