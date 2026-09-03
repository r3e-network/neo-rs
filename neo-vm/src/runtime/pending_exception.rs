use alloc::{format, string::String};

#[derive(Debug, Clone)]
pub(crate) enum PendingException<V> {
    Message(String),
    ThrownValue(V),
}

pub(crate) trait PendingExceptionValue {
    fn from_exception_message(message: String) -> Self;
}

impl PendingExceptionValue for crate::StackValue {
    fn from_exception_message(message: String) -> Self {
        Self::ByteString(message.into_bytes())
    }
}

impl<V> PendingException<V> {
    pub(crate) fn message(message: String) -> Self {
        Self::Message(message)
    }

    pub(crate) fn thrown_value(value: V) -> Self {
        Self::ThrownValue(value)
    }

    pub(crate) fn into_catch_item(self) -> V
    where
        V: PendingExceptionValue,
    {
        match self {
            Self::Message(message) => V::from_exception_message(message),
            Self::ThrownValue(value) => value,
        }
    }
}

impl<V: core::fmt::Debug> PendingException<V> {
    pub(crate) fn fault_message(&self) -> String {
        match self {
            Self::Message(message) => message.clone(),
            Self::ThrownValue(value) => format!("exception: {:?}", value),
        }
    }

    pub(crate) fn into_fault_message(self) -> String {
        match self {
            Self::Message(message) => message,
            Self::ThrownValue(value) => format!("THROW: {:?}", value),
        }
    }
}
