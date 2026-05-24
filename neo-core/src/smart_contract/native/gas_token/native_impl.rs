use super::GasToken;
use crate::error::{CoreError, CoreResult};
use crate::impl_native_contract;
use crate::persistence::i_read_only_store::IReadOnlyStoreGeneric;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::manifest::ContractEventDescriptor;
use crate::smart_contract::native::fungible_token::PREFIX_ACCOUNT as ACCOUNT_PREFIX;
use crate::smart_contract::native::{
    LedgerContract, NativeContract, NativeHelpers, NeoToken, PolicyContract,
};
use crate::smart_contract::storage_key::StorageKey;
use num_bigint::BigInt;

impl NativeContract for GasToken {
    impl_native_contract!(*super::GAS_HASH, Self::NAME, methods);

    fn id(&self) -> i32 {
        Self::ID
    }

    fn is_active(&self, _settings: &ProtocolSettings, _block_height: u32) -> bool {
        true
    }

    fn supported_standards(&self, _settings: &ProtocolSettings, _block_height: u32) -> Vec<String> {
        Self::supported_standards_metadata()
    }

    fn events(
        &self,
        _settings: &ProtocolSettings,
        _block_height: u32,
    ) -> Vec<ContractEventDescriptor> {
        Self::event_descriptors()
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        if snapshot
            .as_ref()
            .try_get(&Self::total_supply_key())
            .is_some()
        {
            return Ok(());
        }

        let validators = engine.protocol_settings().standby_validators();
        if validators.is_empty() {
            return Ok(());
        }
        let account = NativeHelpers::get_bft_address(&validators);
        let amount = BigInt::from(engine.protocol_settings().initial_gas_distribution);
        self.mint(engine, &account, &amount, false)
    }

    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let block = engine.get_persisting_block()?;
        let block_hash = block.header.clone().hash();

        let mut total_network_fee: i64 = 0;
        let snapshot = engine.snapshot_cache();
        let snapshot_ref = snapshot.as_ref();
        let policy = PolicyContract::new();

        // Burn system fee + network fee from each transaction sender
        for tx in &block.transactions {
            let sender = match tx.sender() {
                Some(s) => s,
                None => continue, // Skip transactions without a sender
            };
            let total_fee = tx.system_fee() + tx.network_fee();
            let burn_amount = BigInt::from(total_fee);
            let pre_balance = self.balance_of_snapshot(snapshot_ref, &sender);
            let sender_balance_key =
                StorageKey::create_with_uint160(Self::ID, ACCOUNT_PREFIX, &sender).to_array();
            if Self::is_watched_account(&sender) {
                tracing::info!(
                    target: "neo",
                    block_index = block.index(),
                    block_hash = %block_hash,
                    tx_hash = %tx.hash(),
                    sender = %sender,
                    sender_balance_key = %format!("0x{}", hex::encode(&sender_balance_key)),
                    system_fee = tx.system_fee(),
                    network_fee = tx.network_fee(),
                    burn_amount = %burn_amount,
                    pre_balance = %pre_balance,
                    "watched GAS on_persist sender burn preparation"
                );
            }
            if pre_balance < burn_amount {
                // Diagnostic: dump key state to understand why sender has insufficient GAS
                let total_supply = self.total_supply_snapshot(snapshot_ref);
                let bft_account = {
                    let validators = engine.protocol_settings().standby_validators();
                    crate::smart_contract::native::helpers::NativeHelpers::get_bft_address(
                        &validators,
                    )
                };
                let bft_balance = self.balance_of_snapshot(snapshot_ref, &bft_account);
                let ledger = LedgerContract::new();
                let current_idx = ledger.current_index(snapshot_ref).unwrap_or(999999);
                tracing::warn!(
                    target: "neo",
                    block_index = block.index(),
                    block_hash = %block_hash,
                    tx_hash = %tx.hash(),
                    sender = %sender,
                    system_fee = tx.system_fee(),
                    network_fee = tx.network_fee(),
                    burn_amount = %burn_amount,
                    pre_balance = %pre_balance,
                    total_supply = %total_supply,
                    bft_address = %bft_account,
                    bft_balance = %bft_balance,
                    ledger_current_index = current_idx,
                    "insufficient sender balance before gas burn"
                );
            }
            self.burn(engine, &sender, &burn_amount).map_err(|err| {
                CoreError::native_contract(format!(
                    "GasToken burn failed at block {} ({}), tx {} sender {} (system_fee={}, network_fee={}, burn={}, balance={}): {}",
                    block.index(),
                    block_hash,
                    tx.hash(),
                    sender,
                    tx.system_fee(),
                    tx.network_fee(),
                    burn_amount,
                    pre_balance,
                    err
                ))
            })?;
            total_network_fee += tx.network_fee();

            total_network_fee -= Self::notary_fee_deduction(&policy, snapshot_ref, tx)?;
        }

        // Mint total network fee to the primary consensus node
        if total_network_fee > 0 {
            let neo_token = NeoToken::new();
            let validators = neo_token
                .get_next_block_validators_snapshot(
                    snapshot_ref,
                    usize::try_from(engine.protocol_settings().validators_count.max(0))
                        .unwrap_or(0),
                    engine.protocol_settings(),
                )
                .unwrap_or_else(|_| engine.protocol_settings().standby_validators());

            if !validators.is_empty() {
                let primary_index = block.header.primary_index as usize;
                if primary_index < validators.len() {
                    let primary_validator = &validators[primary_index];
                    let primary_account =
                        crate::smart_contract::Contract::create_signature_contract(
                            primary_validator.clone(),
                        )
                        .script_hash();
                    let mint_amount = BigInt::from(total_network_fee);
                    self.mint(engine, &primary_account, &mint_amount, false)?;
                }
            }
        }

        Ok(())
    }
}
