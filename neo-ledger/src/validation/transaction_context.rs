use neo_native_contracts::{GasToken, NativeContract, Notary};
use neo_primitives::{UInt160, UInt256};
use neo_protocol::payloads::{OracleResponse, Transaction, TransactionAttribute};
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;
use num_traits::Zero;
use std::collections::HashMap;
use std::sync::Arc;

type Payer = (UInt160, Option<UInt160>);
type BalanceProvider = Arc<dyn Fn(&DataCache, &UInt160) -> BigInt + Send + Sync>;

pub struct TransactionVerificationContext {
    sender_fee: HashMap<Payer, BigInt>,
    oracle_responses: HashMap<u64, UInt256>,
    balance_provider: BalanceProvider,
}

impl TransactionVerificationContext {
    pub fn new() -> Self {
        Self::with_balance_provider(default_balance_provider())
    }

    pub fn with_balance_provider<F>(provider: F) -> Self
    where
        F: Fn(&DataCache, &UInt160) -> BigInt + Send + Sync + 'static,
    {
        let balance_provider: BalanceProvider = Arc::new(provider);
        Self {
            sender_fee: HashMap::new(),
            oracle_responses: HashMap::new(),
            balance_provider,
        }
    }

    pub fn total_fee_for_sender(&self, sender: &UInt160) -> Option<&BigInt> {
        self.sender_fee.get(&(*sender, None))
    }

    pub fn payer(tx: &Transaction) -> Option<(UInt160, Option<UInt160>)> {
        let primary = tx.sender()?;
        if primary == Notary::new().hash() && tx.signers().len() >= 2 {
            Some((primary, Some(tx.signers()[1].account)))
        } else {
            Some((primary, None))
        }
    }

    pub fn add_transaction(&mut self, tx: &Transaction) {
        let Some(payer) = Self::payer(tx) else {
            return;
        };
        let fee = Self::fee_amount(tx);
        self.sender_fee
            .entry(payer)
            .and_modify(|existing| *existing += &fee)
            .or_insert(fee);
        if let Some(oracle) = Self::oracle_response(tx) {
            self.oracle_responses.insert(oracle.id, tx.hash());
        }
    }

    pub fn remove_transaction(&mut self, tx: &Transaction) {
        let Some(payer) = Self::payer(tx) else {
            return;
        };
        let fee = Self::fee_amount(tx);
        if let Some(existing) = self.sender_fee.get_mut(&payer) {
            *existing -= &fee;
            if existing.is_zero() {
                self.sender_fee.remove(&payer);
            }
        }
        if let Some(oracle) = Self::oracle_response(tx) {
            self.oracle_responses.remove(&oracle.id);
        }
    }

    pub fn check_transaction<'a, I>(
        &self,
        tx: &Transaction,
        conflicting_txs: I,
        snapshot: &DataCache,
    ) -> bool
    where
        I: IntoIterator<Item = &'a Transaction>,
    {
        let Some(payer) = Self::payer(tx) else {
            return true;
        };
        let mut expected_fee = Self::fee_amount(tx);
        if let Some(existing) = self.sender_fee.get(&payer) {
            expected_fee += existing.clone();
        }
        for conflict in conflicting_txs {
            if Self::payer(conflict) == Some(payer) {
                expected_fee -= Self::fee_amount(conflict);
            }
        }
        let balance = match payer.1 {
            Some(secondary) => Notary::new().balance_of(snapshot, &secondary),
            None => self.balance(snapshot, &payer.0),
        };
        if balance < expected_fee {
            return false;
        }
        if let Some(oracle) = Self::oracle_response(tx)
            && self.oracle_responses.contains_key(&oracle.id)
        {
            return false;
        }
        true
    }

    fn balance(&self, snapshot: &DataCache, account: &UInt160) -> BigInt {
        (self.balance_provider)(snapshot, account)
    }

    fn fee_amount(tx: &Transaction) -> BigInt {
        BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee())
    }

    fn oracle_response(tx: &Transaction) -> Option<&OracleResponse> {
        tx.attributes().iter().find_map(|attr| match attr {
            TransactionAttribute::OracleResponse(response) => Some(response),
            _ => None,
        })
    }
}

impl Default for TransactionVerificationContext {
    fn default() -> Self {
        Self::new()
    }
}

fn default_balance_provider() -> impl Fn(&DataCache, &UInt160) -> BigInt + Send + Sync + 'static {
    let gas = GasToken::new();
    move |snapshot: &DataCache, account: &UInt160| gas.balance_of_snapshot(snapshot, account)
}
