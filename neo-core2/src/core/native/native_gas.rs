use std::sync::Arc;
use std::collections::HashMap;
use std::error::Error;
use num_bigint::BigInt;

use crate::config::Hardfork;
use crate::core::dao::Simple as SimpleDAO;
use crate::core::interop::{Context, HFSpecificContractMD};
use crate::core::native::nativenames;
use crate::core::state::StorageItem;
use crate::core::transaction::Transaction;
use crate::crypto::hash;
use crate::crypto::keys::PublicKey;
use crate::smartcontract;
use crate::util::Uint160;

// GAS represents GAS native contract.
pub struct GAS {
    nep17_token_native: NEP17TokenNative,
    neo: Arc<NEO>,
    policy: Arc<Policy>,

    initial_supply: i64,
    p2p_sig_extensions_enabled: bool,
}

const GAS_CONTRACT_ID: i32 = -6;

// GASFactor is a divisor for finding GAS integral value.
const GAS_FACTOR: i64 = NEO_TOTAL_SUPPLY;

impl GAS {
    // new_gas returns GAS native contract.
    pub fn new(init: i64, p2p_sig_extensions_enabled: bool) -> Arc<Self> {
        let gas = Arc::new(Self {
            nep17_token_native: NEP17TokenNative::new(nativenames::GAS, GAS_CONTRACT_ID),
            neo: Arc::new(NEO::default()),
            policy: Arc::new(Policy::default()),
            initial_supply: init,
            p2p_sig_extensions_enabled,
        });

        {
            let gas_ref = Arc::get_mut(&mut gas).unwrap();
            gas_ref.nep17_token_native.symbol = "GAS".to_string();
            gas_ref.nep17_token_native.decimals = 8;
            gas_ref.nep17_token_native.factor = GAS_FACTOR;
            gas_ref.nep17_token_native.inc_balance = Arc::new(gas_ref.increase_balance);
            gas_ref.nep17_token_native.bal_from_bytes = Arc::new(gas_ref.balance_from_bytes);
        }

        gas.build_hf_specific_md(gas.active_in());
        gas
    }

    fn increase_balance(&self, _: &Context, _: Uint160, si: &mut StorageItem, amount: &BigInt, check_bal: Option<&BigInt>) -> Result<(), Box<dyn Error>> {
        let mut acc = state::nep17_balance_from_bytes(si)?;
        match amount.sign() {
            num_bigint::Sign::NoSign => {
                if let Some(check) = check_bal {
                    if acc.balance < *check {
                        return Err("insufficient funds".into());
                    }
                }
            },
            num_bigint::Sign::Minus => {
                if acc.balance.abs() < amount.abs() {
                    return Err("insufficient funds".into());
                }
            },
            _ => {}
        }
        acc.balance += amount;
        if !acc.balance.is_zero() {
            *si = acc.to_bytes();
        } else {
            *si = Vec::new();
        }
        Ok(())
    }

    fn balance_from_bytes(&self, si: &StorageItem) -> Result<BigInt, Box<dyn Error>> {
        let acc = state::nep17_balance_from_bytes(si)?;
        Ok(acc.balance)
    }

    // Initialize initializes a GAS contract.
    pub fn initialize(&self, ic: &Context, hf: &Hardfork, new_md: &HFSpecificContractMD) -> Result<(), Box<dyn Error>> {
        if hf != &self.active_in() {
            return Ok(());
        }

        self.nep17_token_native.initialize(ic)?;
        let (_, total_supply) = self.nep17_token_native.get_total_supply(&ic.dao);
        if !total_supply.is_zero() {
            return Err("already initialized".into());
        }
        let h = get_standby_validators_hash(ic)?;
        self.mint(ic, h, BigInt::from(self.initial_supply), false);
        Ok(())
    }

    // initialize_cache implements the Contract interface.
    pub fn initialize_cache(&self, _block_height: u32, _d: &SimpleDAO) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    // on_persist implements the Contract interface.
    pub fn on_persist(&self, ic: &Context) -> Result<(), Box<dyn Error>> {
        if ic.block.transactions.is_empty() {
            return Ok(());
        }
        for tx in &ic.block.transactions {
            let abs_amount = BigInt::from(tx.system_fee + tx.network_fee);
            self.burn(ic, tx.sender(), abs_amount);
        }
        let validators = self.neo.get_next_block_validators_internal(&ic.dao);
        let primary = validators[ic.block.primary_index].get_script_hash();
        let mut net_fee = 0i64;
        for tx in &ic.block.transactions {
            net_fee += tx.network_fee;
            if self.p2p_sig_extensions_enabled {
                if let Some(attr) = tx.get_attribute(transaction::NotaryAssistedT) {
                    if let transaction::AttributeValue::NotaryAssisted(na) = attr.value {
                        net_fee -= (na.n_keys as i64 + 1) * self.policy.get_attribute_fee_internal(&ic.dao, transaction::NotaryAssistedT);
                    }
                }
            }
        }
        self.mint(ic, primary, BigInt::from(net_fee), false);
        Ok(())
    }

    // post_persist implements the Contract interface.
    pub fn post_persist(&self, _ic: &Context) -> Result<(), Box<dyn Error>> {
        Ok(())
    }

    // active_in implements the Contract interface.
    pub fn active_in(&self) -> Hardfork {
        Hardfork::default()
    }

    // balance_of returns native GAS token balance for the acc.
    pub fn balance_of(&self, d: &SimpleDAO, acc: Uint160) -> BigInt {
        self.balance_of_internal(d, acc)
    }
}

fn get_standby_validators_hash(ic: &Context) -> Result<Uint160, Box<dyn Error>> {
    let cfg = ic.chain.get_config();
    let committee: Vec<PublicKey> = cfg.standby_committee.iter()
        .map(|s| PublicKey::from_str(s))
        .collect::<Result<_, _>>()?;
    let s = smartcontract::create_default_multi_sig_redeem_script(&committee[..cfg.get_num_of_cns(0)])?;
    Ok(hash::hash160(&s))
}
