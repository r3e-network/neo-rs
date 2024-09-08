
use neo::prelude::*;
use neo::sys::{ContractTask, ApplicationEngine};
use neo::types::{UInt160, Hardfork};
use neo::vm::types::{ProtocolSettings, Transaction};
use neo::smart_contract::{Contract, FungibleToken, AccountState};

/// Represents the GAS token in the NEO system.
pub struct GasToken {
    token: FungibleToken<AccountState>,
}

impl NativeContract for GasToken {
    fn on_persist(&self, engine: &mut ApplicationEngine) -> ContractTask {
        self.on_persist_async(engine)
    }
}

impl GasToken {
    pub fn new() -> Self {
        GasToken {
            token: FungibleToken::new("GAS".to_string(), 8),
        }
    }

    #[contract_method(cpu_fee = 1 << 15)]
    pub fn symbol(&self) -> String {
        self.token.symbol()
    }

    #[contract_method(cpu_fee = 1 << 15)]
    pub fn decimals(&self) -> u8 {
        self.token.decimals()
    }

    pub(crate) fn initialize(&self, engine: &mut ApplicationEngine, hardfork: Option<Hardfork>) -> ContractTask {
        if hardfork == Some(self.active_in()) {
            let account = Contract::get_bft_address(&engine.protocol_settings().standby_validators);
            self.token.mint(engine, &account, engine.protocol_settings().initial_gas_distribution, false)
        } else {
            ContractTask::completed()
        }
    }

    async fn on_persist_async(&self, engine: &mut ApplicationEngine) -> Result<(), String> {
        let mut total_network_fee = 0;
        for tx in engine.persisting_block().transactions() {
            self.token.burn(engine, &tx.sender(), tx.system_fee() + tx.network_fee()).await?;
            total_network_fee += tx.network_fee();
        }
        let validators = NEO::get_next_block_validators(&engine.snapshot_cache, engine.protocol_settings().validators_count);
        let primary = Contract::create_signature_redeem_script(&validators[engine.persisting_block().primary_index]).to_script_hash();
        self.token.mint(engine, &primary, total_network_fee, false).await
    }

    fn active_in(&self) -> Hardfork {
        Hardfork::HF_Cockatrice
    }
}

// Note: The `main` function is not needed in Neo smart contracts.
// Entry points are defined by public methods in the contract struct.
