use super::*;

#[tokio::test]
async fn test_commit_new() {
    let sig = vec![0u8; 64];
    let msg = CommitMessage::new(100, 0, 1, sig.clone());

    assert_eq!(msg.block_index, 100);
    assert_eq!(msg.view_number, 0);
    assert_eq!(msg.validator_index, 1);
    assert_eq!(msg.signature, sig);
}

#[tokio::test]
async fn test_commit_serialize() {
    let sig = vec![0u8; 64];
    let msg = CommitMessage::new(100, 0, 1, sig);
    let data = msg.serialize();

    assert_eq!(data.len(), 64);
}

#[tokio::test]
async fn test_commit_validate() {
    let valid_sig = vec![0u8; 64];
    let msg = CommitMessage::new(100, 0, 1, valid_sig);
    assert!(msg.validate().is_ok());

    let invalid_sig = vec![0u8; 32];
    let msg = CommitMessage::new(100, 0, 1, invalid_sig);
    assert!(msg.validate().is_err());
}
