use super::error::AkkaError;
use std::collections::VecDeque;
use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Directives returned by supervision strategies when an actor fails.
#[derive(Debug, Clone)]
pub enum SupervisorDirective {
    Stop(String),
    Resume,
    Restart,
    Escalate,
}

impl SupervisorDirective {
    pub fn stop<E: ToString>(reason: E) -> Self {
        SupervisorDirective::Stop(reason.to_string())
    }
}

/// Supervision strategy equivalent to Akka.NET's behaviour.
#[derive(Clone)]
pub struct SupervisorStrategy {
    policy: RestartPolicy,
    decider: Arc<dyn Fn(&AkkaError) -> SupervisorDirective + Send + Sync>,
}

impl fmt::Debug for SupervisorStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SupervisorStrategy")
            .field("policy", &self.policy)
            .finish()
    }
}

impl SupervisorStrategy {
    /// Creates a one-for-one strategy with an optional restart budget and a custom decider.
    pub fn one_for_one<F>(max_retries: Option<usize>, within: Option<Duration>, decider: F) -> Self
    where
        F: Fn(&AkkaError) -> SupervisorDirective + Send + Sync + 'static,
    {
        Self {
            policy: RestartPolicy::OneForOne {
                max_retries,
                within,
            },
            decider: Arc::new(decider),
        }
    }

    pub(crate) fn decide(
        &self,
        error: &AkkaError,
        tracker: &mut FailureTracker,
    ) -> SupervisorDirective {
        let mut directive = (self.decider)(error);

        if let SupervisorDirective::Restart = directive {
            match &self.policy {
                RestartPolicy::OneForOne {
                    max_retries,
                    within,
                } => {
                    if let Some(limit) = max_retries {
                        let count = tracker.record_failure(*limit, *within);
                        if count > *limit {
                            directive = SupervisorDirective::stop(
                                "restart retries exceeded configured limit",
                            );
                        }
                    }
                }
            }
        }

        directive
    }
}

#[derive(Debug, Clone)]
enum RestartPolicy {
    OneForOne {
        max_retries: Option<usize>,
        within: Option<Duration>,
    },
}

#[derive(Debug, Default)]
pub(crate) struct FailureTracker {
    failures: VecDeque<Instant>,
}

impl FailureTracker {
    pub fn new() -> Self {
        Self {
            failures: VecDeque::new(),
        }
    }

    pub fn record_failure(&mut self, limit: usize, window: Option<Duration>) -> usize {
        let now = Instant::now();

        if let Some(window) = window {
            while let Some(front) = self.failures.front() {
                if now.duration_since(*front) > window {
                    self.failures.pop_front();
                } else {
                    break;
                }
            }
        } else if self.failures.len() > limit {
            self.failures.pop_front();
        }

        self.failures.push_back(now);
        self.failures.len()
    }
}
