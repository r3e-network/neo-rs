//! Protocol compliance tests for Neo N3 v3.9.1

mod protocol_compliance;

#[cfg(test)]
mod tests {
    use super::protocol_compliance::*;

    #[test]
    fn test_harness_initialization() {
        let harness = test_harness::ProtocolTestHarness::new();
        assert_eq!(harness.test_vectors.len(), 0);
    }

    #[test]
    fn test_state_root_comparison_match() {
        let root1 = vec![1, 2, 3, 4];
        let root2 = vec![1, 2, 3, 4];
        let result = state_comparison::compare_state_roots(&root1, &root2);
        assert!(result.is_compliant());
    }

    #[test]
    fn test_state_root_comparison_mismatch() {
        let root1 = vec![1, 2, 3, 4];
        let root2 = vec![5, 6, 7, 8];
        let result = state_comparison::compare_state_roots(&root1, &root2);
        assert!(!result.is_compliant());
    }
}
