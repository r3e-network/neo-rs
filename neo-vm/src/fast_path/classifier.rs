//! Transaction complexity classifier for fast path routing.
//!
//! Analyzes transaction scripts to determine execution strategy:
//! - **SimpleTransfer**: Standard NEP-17 token transfers (GAS/NEO)
//! - **BlockScript**: Fixed OnPersist/PostPersist system scripts
//! - **ComplexContract**: Everything else requiring full VM

use super::error::{FastPathError, FastPathResult};
use crate::OpCode;

/// Transaction complexity classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionComplexity {
    /// Simple NEP-17 transfer (GAS/NEO) - can use fast path
    SimpleTransfer,

    /// Fixed block script (OnPersist/PostPersist) - can use fast path
    BlockScript,

    /// Complex smart contract call - requires full VM
    ComplexContract,
}

/// Block script type for system transactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockScriptType {
    /// OnPersist system script
    OnPersist,

    /// PostPersist system script
    PostPersist,
}

/// Fast transaction classifier.
pub struct TransactionClassifier;

impl TransactionClassifier {
    /// Classifies a transaction script for execution routing.
    ///
    /// This is the entry point for determining whether a transaction can use
    /// the fast path or requires full VM execution.
    pub fn classify(script: &[u8]) -> TransactionComplexity {
        // Check for NEP-17 transfer pattern
        if Self::is_nep17_transfer(script) {
            return TransactionComplexity::SimpleTransfer;
        }

        // Check for block script patterns
        if Self::is_block_script(script).is_some() {
            return TransactionComplexity::BlockScript;
        }

        // Everything else requires full VM
        TransactionComplexity::ComplexContract
    }

    /// Identifies standard NEP-17 transfer pattern.
    ///
    /// Standard NEP-17 transfer script structure:
    /// ```text
    /// PUSHDATA1 <from_address>      (20 bytes)
    /// PUSHDATA1 <to_address>        (20 bytes)
    /// PUSHDATA1 <amount>            (variable, typically 1-8 bytes)
    /// PUSHNULL
    /// PUSH4
    /// PACK
    /// PUSHDATA1 "transfer"          (8 bytes)
    /// PUSHDATA1 <contract_hash>     (20 bytes)
    /// SYSCALL System.Contract.Call
    /// ```
    pub fn is_nep17_transfer(script: &[u8]) -> bool {
        // NEP-17 transfer scripts are typically 50-150 bytes
        if script.len() < 50 || script.len() > 150 {
            return false;
        }

        // Look for key signatures of NEP-17 transfer
        let has_transfer = script.windows(8).any(|w| w == b"transfer");
        let has_syscall = script.contains(&(OpCode::SYSCALL as u8));
        let has_pack = script.contains(&(OpCode::PACK as u8));
        let has_push4 = script.contains(&(OpCode::PUSH4 as u8));

        // All key elements must be present
        has_transfer && has_syscall && has_pack && has_push4
    }

    /// Identifies block scripts (OnPersist/PostPersist).
    pub fn is_block_script(script: &[u8]) -> Option<BlockScriptType> {
        // Block scripts are very short and fixed
        if script.len() > 50 {
            return None;
        }

        if Self::is_onpersist_script(script) {
            return Some(BlockScriptType::OnPersist);
        }

        if Self::is_postpersist_script(script) {
            return Some(BlockScriptType::PostPersist);
        }

        None
    }

    /// Checks for OnPersist script pattern.
    ///
    /// OnPersist script structure:
    /// ```text
    /// PUSHDATA1 0x09 "onPersist"
    /// SYSCALL System.Contract.Call
    /// ```
    fn is_onpersist_script(script: &[u8]) -> bool {
        if script.len() < 11 {
            return false;
        }

        // Check for PUSHDATA1 with length 9
        if script[0] != OpCode::PUSHDATA1 as u8 || script[1] != 0x09 {
            return false;
        }

        // Check for "onPersist" string
        &script[2..11] == b"onPersist"
    }

    /// Checks for PostPersist script pattern.
    ///
    /// PostPersist script structure:
    /// ```text
    /// PUSHDATA1 0x0B "postPersist"
    /// SYSCALL System.Contract.Call
    /// ```
    fn is_postpersist_script(script: &[u8]) -> bool {
        if script.len() < 13 {
            return false;
        }

        // Check for PUSHDATA1 with length 11
        if script[0] != OpCode::PUSHDATA1 as u8 || script[1] != 0x0B {
            return false;
        }

        // Check for "postPersist" string
        &script[2..13] == b"postPersist"
    }

    /// Validates that a script matches NEP-17 transfer pattern strictly.
    ///
    /// This is used before attempting fast path execution to ensure
    /// the script structure is exactly what we expect.
    pub fn validate_nep17_pattern(script: &[u8]) -> FastPathResult<()> {
        if !Self::is_nep17_transfer(script) {
            return Err(FastPathError::InvalidPattern(
                "Script does not match NEP-17 transfer pattern".to_string()
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_short_script() {
        let script = vec![OpCode::RET as u8];
        assert_eq!(
            TransactionClassifier::classify(&script),
            TransactionComplexity::ComplexContract
        );
    }

    #[test]
    fn test_classify_nep17_pattern() {
        let mut script = Vec::new();

        // Simulate NEP-17 transfer pattern
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20); // from address length
        script.extend_from_slice(&[0u8; 20]); // from address

        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20); // to address length
        script.extend_from_slice(&[0u8; 20]); // to address

        script.push(OpCode::PUSHDATA1 as u8);
        script.push(1); // amount length
        script.push(100); // amount

        script.push(OpCode::PUSHNULL as u8);
        script.push(OpCode::PUSH4 as u8);
        script.push(OpCode::PACK as u8);

        script.push(OpCode::PUSHDATA1 as u8);
        script.push(8); // "transfer" length
        script.extend_from_slice(b"transfer");

        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20); // contract hash length
        script.extend_from_slice(&[0u8; 20]); // contract hash

        script.push(OpCode::SYSCALL as u8);

        assert_eq!(
            TransactionClassifier::classify(&script),
            TransactionComplexity::SimpleTransfer
        );
    }

    #[test]
    fn test_classify_onpersist() {
        let mut script = Vec::new();
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(0x09);
        script.extend_from_slice(b"onPersist");
        script.push(OpCode::SYSCALL as u8);

        assert_eq!(
            TransactionClassifier::classify(&script),
            TransactionComplexity::BlockScript
        );

        assert_eq!(
            TransactionClassifier::is_block_script(&script),
            Some(BlockScriptType::OnPersist)
        );
    }

    #[test]
    fn test_classify_postpersist() {
        let mut script = Vec::new();
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(0x0B);
        script.extend_from_slice(b"postPersist");
        script.push(OpCode::SYSCALL as u8);

        assert_eq!(
            TransactionClassifier::classify(&script),
            TransactionComplexity::BlockScript
        );

        assert_eq!(
            TransactionClassifier::is_block_script(&script),
            Some(BlockScriptType::PostPersist)
        );
    }
}
