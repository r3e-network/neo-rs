// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::vec::Vec;

use neo_crypto::ecc256::PublicKey;

#[derive(Debug, Clone, thiserror::Error)]
pub enum TxVerifyError {
    #[error("tx-verify: {0} exceed tx max size")]
    ExceedMaxSize(usize),

    #[error("tx-verify: invalid script at {0}:{1}")]
    InvalidScript(u32, u8),

    #[error("tx-verify: invalid witnesses")]
    InvalidWitnesses,

    #[error("tx-verify: invalid sign")]
    InvalidSign,
}

pub struct MultiSigners {
    pub keys: Vec<PublicKey>,
    pub signers: u16, // i.e n
}

pub enum WitnessSigner {
    None,
    Single(PublicKey),
    Multi(MultiSigners),
}

pub trait TxVerify {
    fn preverify_tx(&self, network: u32) -> Result<Vec<WitnessSigner>, TxVerifyError>;
}
