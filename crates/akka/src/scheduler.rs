use crate::actor_ref::ActorRef;
use std::any::Any;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::sync::Notify;
use tokio::task;
use tokio::time;

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
        let token = Arc::new(CancelToken::new());
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
                _ = cancel.notified() => {}
            }
        });

        ScheduleHandle { token }
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
        let token = Arc::new(CancelToken::new());
        let cancel = token.clone();

        self.runtime.spawn(async move {
            if !initial_delay.is_zero() {
                tokio::select! {
                    _ = time::sleep(initial_delay) => {},
                    _ = cancel.notified() => return,
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
                    _ = cancel.notified() => break,
                }
            }
        });

        ScheduleHandle { token }
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

#[derive(Clone)]
pub struct ScheduleHandle {
    token: Arc<CancelToken>,
}

impl ScheduleHandle {
    pub fn cancel(&self) {
        self.token.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.token.is_cancelled()
    }
}

impl Drop for ScheduleHandle {
    fn drop(&mut self) {
        self.cancel();
    }
}

struct CancelToken {
    cancelled: AtomicBool,
    notify: Notify,
}

impl CancelToken {
    fn new() -> Self {
        Self {
            cancelled: AtomicBool::new(false),
            notify: Notify::new(),
        }
    }

    fn cancel(&self) {
        if !self.cancelled.swap(true, Ordering::SeqCst) {
            self.notify.notify_waiters();
        }
    }

    fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    async fn notified(&self) {
        if self.is_cancelled() {
            return;
        }
        self.notify.notified().await;
    }
}
