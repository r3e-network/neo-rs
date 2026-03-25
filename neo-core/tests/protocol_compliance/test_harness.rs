//! Test harness for running protocol compliance tests

use super::{test_vectors::TestVector, ComplianceResult};

/// Test harness for protocol compliance testing
pub struct ProtocolTestHarness {
    pub test_vectors: Vec<TestVector>,
}

impl ProtocolTestHarness {
    pub fn new() -> Self {
        Self {
            test_vectors: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn load_vectors(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        self.test_vectors = super::test_vectors::load_vectors(path)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn run_all(&self) -> Vec<ComplianceResult> {
        self.test_vectors
            .iter()
            .map(|_v| ComplianceResult::Inconclusive {
                reason: "Not implemented".to_string(),
            })
            .collect()
    }
}
