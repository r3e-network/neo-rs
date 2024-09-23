use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

// EventID represents an event type happening on the chain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum EventID {
    InvalidEventID = 0,
    BlockEventID,
    TransactionEventID,
    NotificationEventID,
    ExecutionEventID,
    NotaryRequestEventID,
    HeaderOfAddedBlockEventID,
    MissedEventID = 255,
}

impl fmt::Display for EventID {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            EventID::BlockEventID => "block_added",
            EventID::TransactionEventID => "transaction_added",
            EventID::NotificationEventID => "notification_from_execution",
            EventID::ExecutionEventID => "transaction_executed",
            EventID::NotaryRequestEventID => "notary_request_event",
            EventID::HeaderOfAddedBlockEventID => "header_of_added_block",
            EventID::MissedEventID => "event_missed",
            _ => "unknown",
        };
        write!(f, "{}", s)
    }
}

impl FromStr for EventID {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "block_added" => Ok(EventID::BlockEventID),
            "transaction_added" => Ok(EventID::TransactionEventID),
            "notification_from_execution" => Ok(EventID::NotificationEventID),
            "transaction_executed" => Ok(EventID::ExecutionEventID),
            "notary_request_event" => Ok(EventID::NotaryRequestEventID),
            "header_of_added_block" => Ok(EventID::HeaderOfAddedBlockEventID),
            "event_missed" => Ok(EventID::MissedEventID),
            _ => Err("invalid stream name".to_string()),
        }
    }
}

impl Serialize for EventID {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for EventID {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        EventID::from_str(&s).map_err(serde::de::Error::custom)
    }
}
