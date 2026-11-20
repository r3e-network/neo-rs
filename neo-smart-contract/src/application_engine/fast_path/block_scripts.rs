//! Block script classification.
//!
//! Identifies the system block-script types (OnPersist, PostPersist) emitted by
//! the protocol. Classification only — block scripts are always run through the
//! canonical engine.

use neo_vm::fast_path::{FastPathError, FastPathResult};

use super::classifier::BlockScriptType;

/// Identifies system block-script types (OnPersist, PostPersist) for classification.
///
/// Does not bypass or accelerate native hook execution. OnPersist and PostPersist
/// must retain their full engine, snapshot, event and fee semantics, so this type
/// intentionally exposes no executor: it only names the script it was given.
pub struct BlockScriptExecutor;

impl BlockScriptExecutor {
    /// Names the block script type of the given bytecode.
    pub fn identify_script_type(script: &[u8]) -> FastPathResult<BlockScriptType> {
        use super::classifier::TransactionClassifier;

        TransactionClassifier::is_block_script(script).ok_or_else(|| {
            FastPathError::NotApplicable("Script is not a recognized block script".to_string())
        })
    }
}

#[cfg(test)]
mod tests {
    use neo_vm::OpCode;

    use super::*;

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
