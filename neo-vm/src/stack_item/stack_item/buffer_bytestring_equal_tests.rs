use super::*;
use crate::stack_item::buffer::Buffer as BufferItem;

#[test]
fn buffer_never_equals_bytestring() {
    // In C# Neo VM, Buffer uses reference equality (compound type).
    // Buffer == ByteString is always false, even with same content.
    let bs = StackItem::ByteString(vec![0x01]);
    let buf = StackItem::Buffer(BufferItem::new(vec![0x01]));
    assert!(
        !bs.equals(&buf).unwrap(),
        "ByteString(01) should NOT equal Buffer(01)"
    );
    assert!(
        !buf.equals(&bs).unwrap(),
        "Buffer(01) should NOT equal ByteString(01)"
    );
}

#[test]
fn buffer_reference_equality() {
    // Same Buffer instance equals itself
    let buf = StackItem::Buffer(BufferItem::new(vec![0x01]));
    assert!(buf.equals(&buf).unwrap(), "Buffer should equal itself");
}
