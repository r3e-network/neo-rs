//! Classification of deprecated `alert` and `reject` peer notifications.
//!
//! Mirrors `neo-core::network::p2p::remote_node::alerts`. Both commands are
//! deprecated on N3: an `alert` is validated, summarized, and dropped; a
//! `reject` is turned into a human-readable reason. The `neo-core` original
//! logged these through `tracing` and hex-encoded binary payloads with the `hex`
//! crate; neither is in this crate's allowed dependency set, so the handlers
//! here are pure — they return a structured disposition/summary the caller logs,
//! and hex encoding is done by hand.

/// Maximum payload size accepted for alert messages before dropping them.
pub const MAX_ALERT_PAYLOAD_BYTES: usize = 4 * 1024;
/// Maximum number of bytes captured from an alert payload for a summary.
pub const MAX_ALERT_LOG_BYTES: usize = 256;

/// Disposition of an inbound `alert` payload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlertDisposition {
    /// The payload was empty and is dropped silently.
    Empty,
    /// The payload exceeded [`MAX_ALERT_PAYLOAD_BYTES`] and is dropped.
    Oversized {
        /// Actual payload length in bytes.
        bytes: usize,
        /// The limit that was exceeded.
        limit: usize,
    },
    /// The payload is a deprecated alert; it is summarized and ignored.
    Ignored {
        /// Actual payload length in bytes.
        bytes: usize,
        /// A sanitized, length-bounded summary of the payload.
        summary: String,
    },
}

/// Classifies an inbound `alert` payload without logging or retaining it.
pub fn classify_alert(data: &[u8]) -> AlertDisposition {
    if data.is_empty() {
        return AlertDisposition::Empty;
    }

    if data.len() > MAX_ALERT_PAYLOAD_BYTES {
        return AlertDisposition::Oversized {
            bytes: data.len(),
            limit: MAX_ALERT_PAYLOAD_BYTES,
        };
    }

    AlertDisposition::Ignored {
        bytes: data.len(),
        summary: summarize_alert_payload(data),
    }
}

/// Derives a human-readable reason from a `reject` payload.
///
/// The first byte is the rejected command opcode; the remainder (when present)
/// is the reason text.
pub fn summarize_reject(data: &[u8]) -> String {
    if data.len() > 1 {
        String::from_utf8_lossy(&data[1..]).to_string()
    } else if !data.is_empty() {
        format!("Command 0x{:02x} rejected", data[0])
    } else {
        "Unknown rejection".to_string()
    }
}

/// Builds a sanitized, length-bounded summary of an alert payload.
///
/// Printable UTF-8 payloads keep newline/tab/CR but drop other control
/// characters; non-UTF-8 payloads are rendered as lowercase `0x`-prefixed hex.
/// Payloads longer than [`MAX_ALERT_LOG_BYTES`] are truncated with an ellipsis.
fn summarize_alert_payload(payload: &[u8]) -> String {
    let capture_len = payload.len().min(MAX_ALERT_LOG_BYTES);
    let slice = &payload[..capture_len];
    let mut summary = match std::str::from_utf8(slice) {
        Ok(text) => text
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\r' | '\t'))
            .collect::<String>()
            .trim()
            .to_string(),
        Err(_) => format!("0x{}", hex_encode(slice)),
    };

    if payload.len() > capture_len {
        summary.push_str("...");
    }

    summary
}

/// Lowercase hex encoding without the `hex` crate.
fn hex_encode(bytes: &[u8]) -> String {
    const HEX_DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX_DIGITS[(byte >> 4) as usize] as char);
        out.push(HEX_DIGITS[(byte & 0x0F) as usize] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarize_alert_payload_strips_control_chars() {
        let payload = b"Node alert:\nRestart\x07 now";
        let summary = summarize_alert_payload(payload);
        assert_eq!(summary, "Node alert:\nRestart now");
    }

    #[test]
    fn summarize_alert_payload_serializes_binary_as_hex() {
        let payload = [0xFFu8, 0x00, 0x34, 0xAB];
        let summary = summarize_alert_payload(&payload);
        assert_eq!(summary, "0xff0034ab");
    }

    #[test]
    fn summarize_alert_payload_truncates_output() {
        let payload = vec![b'a'; MAX_ALERT_LOG_BYTES + 8];
        let summary = summarize_alert_payload(&payload);
        assert!(summary.ends_with("..."));
        assert_eq!(
            summary.len(),
            MAX_ALERT_LOG_BYTES + 3,
            "appends ellipsis when payload is longer than capture window"
        );
    }

    #[test]
    fn hex_encode_matches_expected_lower_case() {
        assert_eq!(hex_encode(&[]), "");
        assert_eq!(hex_encode(&[0x00, 0x0f, 0x10, 0xff]), "000f10ff");
    }

    #[test]
    fn classify_alert_handles_empty_oversized_and_normal() {
        assert_eq!(classify_alert(&[]), AlertDisposition::Empty);

        let oversized = vec![b'x'; MAX_ALERT_PAYLOAD_BYTES + 1];
        assert_eq!(
            classify_alert(&oversized),
            AlertDisposition::Oversized {
                bytes: oversized.len(),
                limit: MAX_ALERT_PAYLOAD_BYTES
            }
        );

        let normal = b"deprecated".to_vec();
        assert_eq!(
            classify_alert(&normal),
            AlertDisposition::Ignored {
                bytes: normal.len(),
                summary: "deprecated".to_string()
            }
        );
    }

    #[test]
    fn summarize_reject_covers_all_shapes() {
        assert_eq!(summarize_reject(&[]), "Unknown rejection");
        assert_eq!(summarize_reject(&[0x2c]), "Command 0x2c rejected");
        assert_eq!(summarize_reject(&[0x2c, b'b', b'a', b'd']), "bad");
    }
}
