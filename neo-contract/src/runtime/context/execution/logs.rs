use alloc::{string::String, vec::Vec};
use core::mem;

use super::ExecutionContext;
use crate::runtime::value::Value;

impl<'a> ExecutionContext<'a> {
    pub fn push_log(&mut self, message: String) {
        self.log.push(message);
    }

    pub fn logs(&self) -> &[String] {
        &self.log
    }

    pub fn push_notification(&mut self, name: String, payload: Vec<Value>) {
        self.notifications.push((name, payload));
    }

    pub fn notifications(&self) -> &[(String, Vec<Value>)] {
        &self.notifications
    }

    pub fn drain_logs(&mut self) -> Vec<String> {
        mem::take(&mut self.log)
    }

    pub fn drain_notifications(&mut self) -> Vec<(String, Vec<Value>)> {
        mem::take(&mut self.notifications)
    }
}
