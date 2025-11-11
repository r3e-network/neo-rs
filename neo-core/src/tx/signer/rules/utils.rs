use neo_base::hash::Hash160;

use crate::h160::H160;

pub(super) fn h160_to_hash160(value: &H160) -> Hash160 {
    let mut buf = [0u8; 20];
    buf.copy_from_slice(value.as_le_bytes());
    Hash160::new(buf)
}
