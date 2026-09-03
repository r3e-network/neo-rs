//! Handling for deprecated alert messages and peer reject notifications.

use super::RemoteNode;
use tracing::{trace, warn};

impl RemoteNode {
    /// Maximum payload size accepted for alert messages before dropping them.
    const MAX_ALERT_PAYLOAD_BYTES: usize = 4 * 1024;
    /// Maximum number of bytes logged from an alert payload to avoid log spam.
    const MAX_ALERT_LOG_BYTES: usize = 256;

    /// Handles reject messages from peers.
    /// Reject messages indicate protocol violations or refused operations.
    pub(super) fn on_reject(&mut self, data: &[u8]) {
        let reason = if data.len() > 1 {
            String::from_utf8_lossy(&data[1..]).to_string()
        } else if !data.is_empty() {
            format!("Command 0x{:02x} rejected", data[0])
        } else {
            "Unknown rejection".to_string()
        };

        warn!(
            target: "neo",
            endpoint = %self.endpoint,
            reason = %reason,
            "peer sent reject message"
        );
    }

    /// Handles alert messages from peers.
    /// Alert commands are deprecated on N3, so we validate and drop them.
    pub(super) fn on_alert(&mut self, data: &[u8]) {
        if data.is_empty() {
            trace!(
                target: "neo",
                endpoint = %self.endpoint,
                "dropping empty alert payload"
            );
            return;
        }

        if data.len() > Self::MAX_ALERT_PAYLOAD_BYTES {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                bytes = data.len(),
                limit = Self::MAX_ALERT_PAYLOAD_BYTES,
                "dropping oversized alert payload"
            );
            return;
        }

        let summary = Self::summarize_alert_payload(data);
        warn!(
            target: "neo",
            endpoint = %self.endpoint,
            bytes = data.len(),
            message = %summary,
            "peer sent deprecated alert command; ignoring message"
        );
    }

    fn summarize_alert_payload(payload: &[u8]) -> String {
        let capture_len = payload.len().min(Self::MAX_ALERT_LOG_BYTES);
        let slice = &payload[..capture_len];
        let mut summary = match std::str::from_utf8(slice) {
            Ok(text) => text
                .chars()
                .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
                .collect::<String>()
                .trim()
                .to_string(),
            Err(_) => format!("0x{}", hex::encode(slice)),
        };

        if payload.len() > capture_len {
            summary.push_str("...");
        }

        summary
    }
}

#[cfg(test)]
mod tests {
    use super::RemoteNode;

    #[test]
    fn summarize_alert_payload_strips_control_chars() {
        let payload = b"Node alert:\nRestart\x07 now";
        let summary = RemoteNode::summarize_alert_payload(payload);
        assert_eq!(summary, "Node alert:\nRestart now");
    }

    #[test]
    fn summarize_alert_payload_serializes_binary_as_hex() {
        let payload = [0xFFu8, 0x00, 0x34, 0xAB];
        let summary = RemoteNode::summarize_alert_payload(&payload);
        assert_eq!(summary, "0xff0034ab");
    }

    #[test]
    fn summarize_alert_payload_truncates_output() {
        let payload = vec![b'a'; RemoteNode::MAX_ALERT_LOG_BYTES + 8];
        let summary = RemoteNode::summarize_alert_payload(&payload);
        assert!(summary.ends_with("..."));
        assert_eq!(
            summary.len(),
            RemoteNode::MAX_ALERT_LOG_BYTES + 3,
            "appends ellipsis when payload is longer than capture window"
        );
    }
}
