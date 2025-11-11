use alloc::vec::Vec;

use neo_base::hash::double_sha256;

use crate::h256::H256;

use super::model::Tx;

pub type TxHash = H256;

pub fn tx_hash(tx: &Tx) -> TxHash {
    let mut buf = Vec::new();
    tx.encode_unsigned(&mut buf);
    H256::from_le_bytes(double_sha256(buf))
}
