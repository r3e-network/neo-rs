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
