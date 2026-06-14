use super::header::Header;
use neo_io::{IoError, impl_serializable};
use serde::{Deserialize, Serialize};

/// Indicates the maximum number of headers sent each time.
pub const MAX_HEADERS_COUNT: usize = 2000;

/// This message is sent to respond to GetHeaders messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadersPayload {
    /// The list of headers.
    pub headers: Vec<Header>,
}

impl HeadersPayload {
    /// Creates a new headers payload.
    pub fn create(headers: Vec<Header>) -> Self {
        Self { headers }
    }
}

impl_serializable! {
    struct HeadersPayload {
        headers: var_array<Header> { max: MAX_HEADERS_COUNT },
    }
    validate(self_ref) {
        if self_ref.headers.is_empty() {
            return Err(IoError::invalid_data("Empty headers list"));
        }
    }
}
