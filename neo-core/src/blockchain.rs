// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use alloc::sync::Arc;

use neo_base::errors;
use neo_type::{multi_sign_contract_cost, sign_contract_cost, ChainConfig};
use crate::contract::{NativeContracts, ORACLE_RESPONSE_SCRIPT};
use crate::store::NeoStates;
use crate::tx::*;

#[allow(unused)]
pub struct BlockChain {
    states: Arc<dyn NeoStates>,
    natives: NativeContracts,
    txpool: TxPool,
    config: ChainConfig,
}

impl BlockChain {
    pub fn new(states: Arc<dyn NeoStates>, natives: NativeContracts, config: ChainConfig) -> Self {
        let max_txpool_size = config.max_txpool_size as usize;
        Self {
            states,
            natives,
            txpool: TxPool::new(max_txpool_size, 0), // payer_index only supports 0 now
            config,
        }
    }

    #[inline]
    pub fn txpool(&self) -> TxPool {
        self.txpool.clone()
    }
}

#[derive(Debug, Clone, errors::Error)]
pub enum PoolTxError {
    #[error("pool-tx: {0}")]
    PreverifyError(#[from] TxVerifyError),

    #[error("pool-tx: already in ledger")]
    AlreadyInLedger,

    #[error("pool-tx: has conflicts")]
    HasConflicts,

    #[error("pool-tx: has expired, current: {0}, until: {1}")]
    HasExpired(u32, u32),

    #[error("pool-tx: too early, current: {0}, until: {1}")]
    TooEarly(u32, u32),

    #[error("pool-tx: fee exceed limit: {0} of {1}")]
    ExeedFeeLimit(u64, &'static str),

    #[error("pool-tx: account is blocked")]
    AccountBlocked,

    #[error("pool-tx: invalid {0} attribute")]
    InvalidAttr(&'static str),

    #[error("pool-tx: {0}")]
    AddTxError(#[from] AddTxError),
}

impl BlockChain {
    pub fn pool_tx(&self, mut tx: Tx) -> Result<(), PoolTxError> {
        let _hash = tx.calc_hash();
        let current = self.states.current_block_index();
        let witness_signers = self.check_tx_basic(current, &tx)?;

        // check blocked
        for signer in tx.signers.iter() {
            if self.natives.policy.is_blocked_account(&signer.account) {
                return Err(PoolTxError::AccountBlocked);
            }
        }

        let attr_fee = self.check_tx_attrs(current, &tx)?;
        let _ = self.check_tx_fee(current, &tx, attr_fee, &witness_signers)?;

        let _ = self.txpool.add_tx(tx, self.states.as_ref())?;
        Ok(())
    }
}

impl BlockChain {
    fn check_tx_basic(&self, current: u32, tx: &Tx) -> Result<Vec<WitnessSigner>, PoolTxError> {
        let hash = tx.hash();
        if self.txpool.contains_tx(&hash) {
            return Err(PoolTxError::AddTxError(AddTxError::AlreadyInPool));
        }

        if self.states.contains_tx(&hash) {
            return Err(PoolTxError::AlreadyInLedger);
        }

        if tx.sysfee > self.config.max_block_sysfee {
            return Err(PoolTxError::ExeedFeeLimit(tx.sysfee, "max_block_sysfee"));
        }

        let witness_signers = tx.preverify_tx(self.config.network)?;
        if tx.valid_until_block <= current {
            return Err(PoolTxError::HasExpired(current, tx.valid_until_block));
        }

        if tx.valid_until_block > current + self.config.max_traceable_blocks {
            return Err(PoolTxError::TooEarly(current, tx.valid_until_block));
        }

        for signer in tx.signers.iter() {
            if self.states.contains_conflict(&hash, &signer.account) {
                return Err(PoolTxError::HasConflicts);
            }
        }

        Ok(witness_signers)
    }

    fn check_tx_fee(
        &self,
        _current: u32,
        tx: &Tx,
        attr_fee: u64,
        witness_signers: &[WitnessSigner],
    ) -> Result<(), PoolTxError> {
        let fee_perbyte = self.states.netfee_perbyte();
        let exec_factor = self.natives.policy.exec_fee_factor();

        let netfee = attr_fee + tx.size() as u64 * fee_perbyte;
        if netfee > tx.netfee {
            return Err(PoolTxError::ExeedFeeLimit(netfee, "tx.netfee"));
        }

        let mut verify_fee = 0u64;
        let remain_fee = (tx.netfee - netfee).min(MAX_VERIFICATION_GAS);
        for signer in witness_signers.iter() {
            match signer {
                WitnessSigner::None => {
                    // TODO: exec InvocationScript
                }
                WitnessSigner::Single(_signer) => {
                    verify_fee += exec_factor * sign_contract_cost();
                }
                WitnessSigner::Multi(signers) => {
                    let nr_keys = signers.keys.len() as u32;
                    let nr_signs = signers.signers as u32;
                    verify_fee += exec_factor * multi_sign_contract_cost(nr_keys, nr_signs);
                }
            }
            if verify_fee > remain_fee {
                return Err(PoolTxError::ExeedFeeLimit(netfee, "verify_fee"));
            }
        }

        Ok(())
    }

    fn check_tx_attrs(&self, current: u32, tx: &Tx) -> Result<u64, PoolTxError> {
        let mut attr_fee = 0;
        for attr in tx.attributes.iter() {
            match attr {
                TxAttr::HighPriority => {
                    let committee = self.natives.neo.committee_address();
                    if tx.has_signer(&committee) {
                        return Err(PoolTxError::InvalidAttr("high-priority"));
                    }
                    attr_fee += self.natives.policy.tx_attr_fee(AttrType::HighPriority);
                }
                TxAttr::OracleResponse(res) => {
                    self.check_oracle_attr(tx, res.id)?;
                    attr_fee += self.natives.policy.tx_attr_fee(AttrType::OracleResponse);
                }
                TxAttr::NotValidBefore(attr) => {
                    if attr.height < current as u64 {
                        return Err(PoolTxError::InvalidAttr("not-valid-before"));
                    }
                    attr_fee += self.natives.policy.tx_attr_fee(AttrType::NotValidBefore);
                }
                TxAttr::Conflicts(attr) => {
                    if self.states.contains_tx(&attr.hash) {
                        return Err(PoolTxError::InvalidAttr("conflicts"));
                    }
                    attr_fee += self.natives.policy.tx_attr_fee(AttrType::Conflicts);
                } // TxAttr::NotaryAssisted(_) => (),
            }
        }

        Ok(attr_fee)
    }

    fn check_oracle_attr(&self, tx: &Tx, req_id: u64) -> Result<(), PoolTxError> {
        if tx.signers.iter().any(|x| x.scopes.scopes() != WitnessScope::None as u8) {
            return Err(PoolTxError::InvalidAttr("oracle with scopes not None"));
        }

        if !tx.script.as_bytes().eq(ORACLE_RESPONSE_SCRIPT) {
            return Err(PoolTxError::InvalidAttr("oracle with invalid script"));
        }

        let Some(req) = self.natives.oracle.oracle_request(req_id) else {
            return Err(PoolTxError::InvalidAttr("oracle with no such request"));
        };

        if tx.netfee + tx.sysfee != req.gas_for_response {
            return Err(PoolTxError::InvalidAttr("oracle with fee mismatch"));
        }

        let Some(oracle) = self.natives.oracle.last_designated() else {
            return Err(PoolTxError::InvalidAttr("oracle with no last_designated"));
        };

        if tx.signers.iter().find(|s| s.account == oracle).is_none() {
            return Err(PoolTxError::InvalidAttr("oracle with no oracle signer"));
        }

        Ok(())
    }
}
