use super::actor_ref::ActorRef;
use std::any::Any;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::task;
use tokio::time;
use tokio_util::sync::{CancellationToken, DropGuard};

#[derive(Clone)]
pub struct Scheduler {
    runtime: Handle,
}

impl Scheduler {
    pub(crate) fn new(runtime: Handle) -> Self {
        Self { runtime }
    }

    pub fn schedule_tell_once<M>(
        &self,
        delay: Duration,
        target: ActorRef,
        message: M,
        sender: Option<ActorRef>,
    ) -> ScheduleHandle
    where
        M: Any + Send + 'static,
    {
        let token = CancellationToken::new();
        let cancel = token.clone();

        self.runtime.spawn(async move {
            if delay.is_zero() {
                if !cancel.is_cancelled() {
                    let _ = target.tell_from(message, sender);
                }
                return;
            }

            tokio::select! {
                _ = time::sleep(delay) => {
                    if !cancel.is_cancelled() {
                        let _ = target.tell_from(message, sender);
                    }
                }
                _ = cancel.cancelled() => {}
            }
        });

        ScheduleHandle::new(token)
    }

    pub fn schedule_tell_repeatedly<M>(
        &self,
        initial_delay: Duration,
        interval: Duration,
        target: ActorRef,
        message: M,
        sender: Option<ActorRef>,
    ) -> ScheduleHandle
    where
        M: Clone + Any + Send + 'static,
    {
        let token = CancellationToken::new();
        let cancel = token.clone();

        self.runtime.spawn(async move {
            if !initial_delay.is_zero() {
                tokio::select! {
                    _ = time::sleep(initial_delay) => {},
                    _ = cancel.cancelled() => return,
                }
            }

            if cancel.is_cancelled() {
                return;
            }

            loop {
                if cancel.is_cancelled() {
                    break;
                }

                let _ = target.tell_from(message.clone(), sender.clone());

                if interval.is_zero() {
                    task::yield_now().await;
                    continue;
                }

                tokio::select! {
                    _ = time::sleep(interval) => {},
                    _ = cancel.cancelled() => break,
                }
            }
        });

        ScheduleHandle::new(token)
    }

    pub fn schedule_tell_once_cancelable<M>(
        &self,
        delay: Duration,
        target: ActorRef,
        message: M,
        sender: Option<ActorRef>,
    ) -> ScheduleHandle
    where
        M: Any + Send + 'static,
    {
        self.schedule_tell_once(delay, target, message, sender)
    }

    pub fn schedule_tell_repeatedly_cancelable<M>(
        &self,
        initial_delay: Duration,
        interval: Duration,
        target: ActorRef,
        message: M,
        sender: Option<ActorRef>,
    ) -> ScheduleHandle
    where
        M: Clone + Any + Send + 'static,
    {
        self.schedule_tell_repeatedly(initial_delay, interval, target, message, sender)
    }
}

#[must_use = "scheduled messages are cancelled when the handle is dropped"]
pub struct ScheduleHandle {
    token: CancellationToken,
    _drop_guard: DropGuard,
}

impl ScheduleHandle {
    fn new(token: CancellationToken) -> Self {
        let drop_guard = token.clone().drop_guard();
        Self {
            token,
            _drop_guard: drop_guard,
        }
    }

    pub fn cancel(&self) {
        self.token.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

impl Clone for ScheduleHandle {
    fn clone(&self) -> Self {
        Self::new(self.token.clone())
    }
}
