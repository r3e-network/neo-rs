use alloc::vec::Vec;

use crate::{
    block::header::Header,
    h160::H160,
    h256::H256,
    script::Script,
    tx::{Tx, Witness},
};

pub(super) fn sample_witness() -> Witness {
    Witness::new(Script::new(vec![0x51]), Script::new(vec![0xAC]))
}

pub(super) fn sample_tx(tag: u8) -> Tx {
    Tx {
        version: 0,
        nonce: tag as u32,
        valid_until_block: 0,
        sysfee: 0,
        netfee: 0,
        signers: Vec::new(),
        attributes: Vec::new(),
        script: Script::new(vec![tag]),
        witnesses: Vec::new(),
    }
}

pub(super) fn sample_header() -> Header {
    Header::new(
        0,
        H256::default(),
        H256::default(),
        1,
        42,
        1,
        0,
        H160::default(),
        vec![sample_witness()],
    )
}
