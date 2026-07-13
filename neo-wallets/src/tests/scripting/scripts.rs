use super::*;
use neo_vm::OpCode;

#[test]
fn signature_invocation_uses_neo_pushdata1_form() {
    let signature = [0xAB; 64];
    let invocation = signature_invocation(&signature).expect("invocation script");

    assert_eq!(invocation.len(), 66);
    assert_eq!(invocation[0], OpCode::PUSHDATA1.byte());
    assert_eq!(invocation[1], 0x40);
    assert_eq!(&invocation[2..], signature);
}

#[test]
fn signature_invocation_rejects_non_64_byte_signature() {
    assert!(signature_invocation(&[0xAB; 63]).is_err());
    assert!(signature_invocation(&[0xAB; 65]).is_err());
}
