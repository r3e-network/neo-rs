//! State comparison utilities for protocol compliance

use crate::protocol_compliance::ComplianceResult;

/// Compare two state roots byte-for-byte
pub fn compare_state_roots(rust_root: &[u8], csharp_root: &[u8]) -> ComplianceResult {
    if rust_root == csharp_root {
        ComplianceResult::Compliant
    } else {
        ComplianceResult::Divergent {
            reason: "State root mismatch".to_string(),
            details: format!(
                "Rust: {}, C#: {}",
                hex::encode(rust_root),
                hex::encode(csharp_root)
            ),
        }
    }
}
