//! Change view reason - Why validators request a view change.

use neo_primitives::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    /// Change view reason enum matching C# `ChangeViewReason` exactly
    pub ChangeViewReason {
        /// The view timed out without a committed block.
        #[default]
        Timeout = 0x0,
        /// A different view was agreed upon by the validators.
        ChangeAgreement = 0x1,
        /// A proposed block referenced a transaction that is not available.
        TxNotFound = 0x2,
        /// A proposed transaction was rejected by policy.
        TxRejectedByPolicy = 0x3,
        /// A proposed transaction was invalid.
        TxInvalid = 0x4,
        /// A proposed block was rejected by policy.
        BlockRejectedByPolicy = 0x5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_view_reason_values() {
        assert_eq!(ChangeViewReason::Timeout as u8, 0x0);
        assert_eq!(ChangeViewReason::ChangeAgreement as u8, 0x1);
        assert_eq!(ChangeViewReason::TxNotFound as u8, 0x2);
        assert_eq!(ChangeViewReason::TxRejectedByPolicy as u8, 0x3);
        assert_eq!(ChangeViewReason::TxInvalid as u8, 0x4);
        assert_eq!(ChangeViewReason::BlockRejectedByPolicy as u8, 0x5);
    }

    #[test]
    fn test_change_view_reason_from_byte() {
        assert_eq!(
            ChangeViewReason::from_byte(0x0),
            Some(ChangeViewReason::Timeout)
        );
        assert_eq!(
            ChangeViewReason::from_byte(0x2),
            Some(ChangeViewReason::TxNotFound)
        );
        assert_eq!(ChangeViewReason::from_byte(0x99), None);
    }

    #[test]
    fn test_change_view_reason_roundtrip() {
        for reason in [
            ChangeViewReason::Timeout,
            ChangeViewReason::ChangeAgreement,
            ChangeViewReason::TxNotFound,
            ChangeViewReason::TxRejectedByPolicy,
            ChangeViewReason::TxInvalid,
            ChangeViewReason::BlockRejectedByPolicy,
        ] {
            let byte = reason.to_byte();
            let recovered = ChangeViewReason::from_byte(byte);
            assert_eq!(recovered, Some(reason));
        }
    }

    #[test]
    fn test_change_view_reason_default() {
        assert_eq!(ChangeViewReason::default(), ChangeViewReason::Timeout);
    }

    #[test]
    fn test_change_view_reason_display() {
        assert_eq!(ChangeViewReason::Timeout.to_string(), "Timeout");
        assert_eq!(ChangeViewReason::TxNotFound.to_string(), "TxNotFound");
    }
}
