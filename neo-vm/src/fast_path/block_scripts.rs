//! Block script fast path execution.
//!
//! Optimized execution for OnPersist and PostPersist system scripts.

use super::classifier::BlockScriptType;
use super::error::{FastPathError, FastPathResult};

/// Classifies native block scripts for the canonical ApplicationEngine path.
///
/// This type intentionally exposes no direct executor: native OnPersist and
/// PostPersist must retain their full engine, snapshot, event and fee semantics.
pub struct BlockScriptExecutor;

impl BlockScriptExecutor {
    /// Identifies the block script type from bytecode.
    pub fn identify_script_type(script: &[u8]) -> FastPathResult<BlockScriptType> {
        use super::classifier::TransactionClassifier;

        TransactionClassifier::is_block_script(script).ok_or_else(|| {
            FastPathError::NotApplicable("Script is not a recognized block script".to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpCode;

    #[test]
    fn test_identify_onpersist() {
        let mut script = Vec::new();
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(0x09);
        script.extend_from_slice(b"onPersist");

        let script_type = BlockScriptExecutor::identify_script_type(&script).unwrap();
        assert_eq!(script_type, BlockScriptType::OnPersist);
    }

    #[test]
    fn test_identify_postpersist() {
        let mut script = Vec::new();
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(0x0B);
        script.extend_from_slice(b"postPersist");

        let script_type = BlockScriptExecutor::identify_script_type(&script).unwrap();
        assert_eq!(script_type, BlockScriptType::PostPersist);
    }

    #[test]
    fn test_identify_invalid() {
        let script = vec![OpCode::RET as u8];

        let result = BlockScriptExecutor::identify_script_type(&script);
        assert!(result.is_err());
    }
}
