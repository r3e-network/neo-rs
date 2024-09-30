// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_base::{encoding::bin::BinEncoder, errors};
use neo_crypto::secp256r1::PublicKey;
use neo_type::{MultiSignVerify, ParseInvocationScript, ScriptHash, SignVerify, ToScriptHash, H160, H160_SIZE, MAX_TX_SIZE};
use crate::contract::{MayMultiSignContract, MaySignContract, MultiSigners};
use crate::tx::{Signer as TxSigner, Tx};

pub const MAX_VERIFICATION_GAS: u64 = 1_5000_0000; // 1.5 GAS
pub const CHECK_SIG_COST: u64 = 1 << 15;

#[derive(Debug, Clone, errors::Error)]
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

pub enum WitnessSigner {
    None,
    Single(PublicKey),
    Multi(MultiSigners),
}

pub trait TxVerify {
    fn preverify_tx(&self, network: u32) -> Result<Vec<WitnessSigner>, TxVerifyError>;
}

impl TxVerify for Tx {
    fn preverify_tx(&self, network: u32) -> Result<Vec<WitnessSigner>, TxVerifyError> {
        let tx = self;
        let size = tx.bin_size();
        if size > MAX_TX_SIZE as usize {
            return Err(TxVerifyError::ExceedMaxSize(size));
        }

        // if tx.signers.len() != tx.witnesses.len() {
        //     return Err(TxVerifyError::InvalidWitnesses);
        // }

        let mut witness_signers = Vec::with_capacity(tx.witnesses.len());
        for (i, signer) in tx.signers.iter().enumerate() {
            let Some(witness) = tx.witnesses.get(i) else {
                witness_signers.push(WitnessSigner::None);
                continue;
            };

            let invocation = witness.invocation_script.as_bytes();
            let verification = witness.verification_script.as_bytes();
            if verification.may_sign_contract() {
                let _ = check_verification_script(verification, signer)?;
                let Some(sign) = invocation.parse_sign() else {
                    return Err(TxVerifyError::InvalidWitnesses);
                };

                let public_key = PublicKey::from_compressed(&verification[2..35])
                    .map_err(|_| TxVerifyError::InvalidWitnesses)?;
                if !tx.verify_sign(&public_key, sign, network) {
                    return Err(TxVerifyError::InvalidSign);
                }

                witness_signers.push(WitnessSigner::Single(public_key));
            } else if let Some(signers) = verification.may_multi_sign_contract() {
                let _ = check_verification_script(verification, signer)?;
                let Some(signs) = invocation.parse_multi_signs() else {
                    return Err(TxVerifyError::InvalidWitnesses);
                };

                if !tx.verify_multi_sign(&signers.keys, &signs, network) {
                    return Err(TxVerifyError::InvalidSign);
                }

                witness_signers.push(WitnessSigner::Multi(signers));
            } else {
                witness_signers.push(WitnessSigner::None);
            }
        }

        Ok(witness_signers)
    }
}

#[inline]
fn check_verification_script(verification: &[u8], signer: &TxSigner) -> Result<(), TxVerifyError> {
    let script_hash = verification.to_script_hash();
    if <ScriptHash as AsRef<[u8; H160_SIZE]>>::as_ref(&script_hash)
        != <H160 as AsRef<[u8; H160_SIZE]>>::as_ref(&signer.account)
    {
        return Err(TxVerifyError::InvalidWitnesses);
    }

    Ok(())
}
