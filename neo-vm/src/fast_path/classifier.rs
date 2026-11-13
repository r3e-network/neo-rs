//! Conservative transaction classifier for execution routing.

use super::error::{FastPathError, FastPathResult};
use super::nep17::Nep17Parser;
use crate::OpCode;

/// Transaction complexity classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionComplexity {
    /// Strictly parsed standard NEP-17 transfer; host eligibility still applies.
    SimpleTransfer,
    /// Fixed block script. These always retain canonical native persist hooks.
    BlockScript,
    /// Everything else requires the full VM.
    ComplexContract,
}

/// Block script type for system transactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockScriptType {
    /// OnPersist system script.
    OnPersist,
    /// PostPersist system script.
    PostPersist,
}

/// Conservative transaction classifier.
pub struct TransactionClassifier;

impl TransactionClassifier {
    /// Classifies a transaction without using substring matching as a safety decision.
    #[must_use]
    pub fn classify(script: &[u8]) -> TransactionComplexity {
        if Nep17Parser::parse_transfer_script(script).is_ok() {
            return TransactionComplexity::SimpleTransfer;
        }
        if Self::is_block_script(script).is_some() {
            return TransactionComplexity::BlockScript;
        }
        TransactionComplexity::ComplexContract
    }

    /// Returns true only for a strictly parsed standard transfer script.
    #[must_use]
    pub fn is_nep17_transfer(script: &[u8]) -> bool {
        Nep17Parser::parse_transfer_script(script).is_ok()
    }

    /// Identifies the legacy fixed block-script prefixes for metrics only.
    /// Production OnPersist/PostPersist dispatch remains in ApplicationEngine.
    #[must_use]
    pub fn is_block_script(script: &[u8]) -> Option<BlockScriptType> {
        if Self::is_onpersist_script(script) {
            return Some(BlockScriptType::OnPersist);
        }
        if Self::is_postpersist_script(script) {
            return Some(BlockScriptType::PostPersist);
        }
        None
    }

    fn is_onpersist_script(script: &[u8]) -> bool {
        script.len() >= 11
            && script.len() <= 50
            && script[0] == OpCode::PUSHDATA1 as u8
            && script[1] == 9
            && &script[2..11] == b"onPersist"
    }

    fn is_postpersist_script(script: &[u8]) -> bool {
        script.len() >= 13
            && script.len() <= 50
            && script[0] == OpCode::PUSHDATA1 as u8
            && script[1] == 11
            && &script[2..13] == b"postPersist"
    }

    /// Validates the strict NEP-17 pattern.
    pub fn validate_nep17_pattern(script: &[u8]) -> FastPathResult<()> {
        Nep17Parser::parse_transfer_script(script)
            .map(|_| ())
            .map_err(|error| {
                FastPathError::InvalidPattern(format!(
                    "Script does not match NEP-17 transfer pattern: {error}"
                ))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fast_path::nep17::standard_transfer_script;

    #[test]
    fn classifies_only_strict_nep17_scripts() {
        assert_eq!(
            TransactionClassifier::classify(&standard_transfer_script(100)),
            TransactionComplexity::SimpleTransfer
        );
        let mut malformed = standard_transfer_script(100);
        malformed.push(OpCode::RET as u8);
        assert_eq!(
            TransactionClassifier::classify(&malformed),
            TransactionComplexity::ComplexContract
        );
    }

    #[test]
    fn substring_collisions_are_not_transfers() {
        let mut script = vec![0; 80];
        script[4..12].copy_from_slice(b"transfer");
        script[20] = OpCode::SYSCALL as u8;
        script[21] = OpCode::PACK as u8;
        script[22] = OpCode::PUSH4 as u8;
        assert!(!TransactionClassifier::is_nep17_transfer(&script));
    }

    #[test]
    fn classifies_block_prefixes_for_metrics() {
        let mut on_persist = vec![OpCode::PUSHDATA1 as u8, 9];
        on_persist.extend_from_slice(b"onPersist");
        assert_eq!(
            TransactionClassifier::is_block_script(&on_persist),
            Some(BlockScriptType::OnPersist)
        );
        let mut post_persist = vec![OpCode::PUSHDATA1 as u8, 11];
        post_persist.extend_from_slice(b"postPersist");
        assert_eq!(
            TransactionClassifier::is_block_script(&post_persist),
            Some(BlockScriptType::PostPersist)
        );
    }
}
