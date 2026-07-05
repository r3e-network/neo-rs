use super::*;

#[tokio::test]
async fn test_prepare_response_new() {
    let hash = UInt256::zero();
    let msg = PrepareResponseMessage::new(100, 0, 1, hash);

    assert_eq!(msg.block_index, 100);
    assert_eq!(msg.view_number, 0);
    assert_eq!(msg.validator_index, 1);
    assert_eq!(msg.preparation_hash, hash);
}

#[tokio::test]
async fn test_prepare_response_serialize() {
    let msg = PrepareResponseMessage::new(100, 0, 1, UInt256::zero());
    let data = msg.serialize();

    assert_eq!(data.len(), 32); // UInt256 is 32 bytes
}

#[tokio::test]
async fn test_prepare_response_validate() {
    let hash = UInt256::zero();
    let msg = PrepareResponseMessage::new(100, 0, 1, hash);

    assert!(msg.validate(&hash).is_ok());

    let different_hash = UInt256::from_bytes(&[1u8; 32]).unwrap();
    assert!(msg.validate(&different_hash).is_err());
}
