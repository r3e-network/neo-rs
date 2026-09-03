//! Protocol Compliance Test Infrastructure
//!
//! This module provides infrastructure for testing Neo N3 v3.9.1 protocol compliance
//! by comparing Rust implementation behavior against C# reference implementation.

pub mod state_comparison;
pub mod test_harness;
pub mod test_vectors;

/// Protocol version being tested
#[allow(dead_code)]
pub const PROTOCOL_VERSION: &str = "3.9.1";

/// Test result indicating compliance status
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub enum ComplianceResult {
    /// Behavior matches C# implementation exactly
    Compliant,
    /// Behavior diverges from C# implementation
    Divergent { reason: String, details: String },
    /// Test could not be executed
    Inconclusive { reason: String },
}

impl ComplianceResult {
    pub fn is_compliant(&self) -> bool {
        matches!(self, ComplianceResult::Compliant)
    }
}
