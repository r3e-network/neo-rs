use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_payloads::VerifiableExt;
use neo_payloads::extensible_payload::ExtensiblePayload;
use tracing::debug;

use crate::service::{BlockchainService, MempoolLike};

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::InventoryExtensible`] command.
    ///
    /// C# `Blockchain.OnNewExtensiblePayload`: the payload must pass
    /// [`Self::verify_extensible`] (height range, whitelisted sender, witness
    /// execution) before it is cached/relayed.
    pub(crate) async fn handle_extensible_inventory(
        &self,
        mut payload: ExtensiblePayload,
        relay: bool,
    ) -> CoreResult<()> {
        let hash = payload.hash();
        if let Some(snapshot) = self.system.store_snapshot() {
            let settings = self.system.settings();
            Self::verify_extensible(
                &payload,
                settings.as_ref(),
                &snapshot,
                self.system.native_contract_provider(),
            )
            .map_err(|error| CoreError::other(format!("extensible payload rejected: {error}")))?;
        }
        if let Err(error) = self.ledger.insert_extensible(payload) {
            return Err(CoreError::other(format!("ledger insert: {error}")));
        }
        debug!(target: "neo", %hash, relay, "extensible payload accepted");
        Ok(())
    }

    /// C# `ExtensiblePayload.Verify` + `Blockchain.UpdateExtensibleWitnessWhiteList`:
    /// the current height must lie in `[valid_block_start, valid_block_end)`, the
    /// sender must be one of {committee address, next-block-validators BFT address,
    /// each validator's signature hash, state-validators BFT address, each state
    /// validator's signature hash}, and the witness must verify under the 0.06-GAS
    /// cap.
    fn verify_extensible(
        payload: &ExtensiblePayload,
        settings: &neo_config::ProtocolSettings,
        snapshot: &neo_storage::DataCache,
        native_contract_provider: Option<
            Arc<dyn neo_execution::native_contract_provider::NativeContractProvider>,
        >,
    ) -> CoreResult<()> {
        let ledger = neo_native_contracts::LedgerContract::new();
        let height = ledger
            .current_index(snapshot)
            .map_err(|e| CoreError::other(e.to_string()))?;
        if height < payload.valid_block_start || height >= payload.valid_block_end {
            return Err(CoreError::other(format!(
                "height {height} outside the valid range [{}, {})",
                payload.valid_block_start, payload.valid_block_end
            )));
        }

        let mut whitelist: std::collections::HashSet<neo_primitives::UInt160> =
            std::collections::HashSet::new();
        if let Ok(Some(committee)) = neo_execution::NativeContract::committee_address(
            &neo_native_contracts::NeoToken::new(),
            snapshot,
        ) {
            whitelist.insert(committee);
        }
        let validators = neo_native_contracts::NeoToken::new()
            .next_block_validators(
                snapshot,
                usize::try_from(settings.validators_count).unwrap_or(0),
            )
            .map_err(|e| CoreError::other(e.to_string()))?;
        if !validators.is_empty() {
            whitelist.insert(
                crate::native_persist::bft_address(&validators)
                    .map_err(|e| CoreError::other(e.to_string()))?,
            );
            for validator in &validators {
                whitelist.insert(neo_primitives::UInt160::from_script(
                    &neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
                        validator.as_bytes(),
                    ),
                ));
            }
        }
        let state_validators = neo_native_contracts::RoleManagement::new()
            .get_designated_by_role_at(snapshot, neo_native_contracts::Role::StateValidator, height)
            .unwrap_or_default();
        if !state_validators.is_empty() {
            whitelist.insert(
                crate::native_persist::bft_address(&state_validators)
                    .map_err(|e| CoreError::other(e.to_string()))?,
            );
            for validator in &state_validators {
                whitelist.insert(neo_primitives::UInt160::from_script(
                    &neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(
                        validator.as_bytes(),
                    ),
                ));
            }
        }
        if !whitelist.contains(&payload.sender) {
            return Err(CoreError::other(
                "sender is not in the extensible witness whitelist",
            ));
        }

        // C# `this.VerifyWitnesses(settings, snapshot, 0_06000000L)`.
        let hashes = payload.script_hashes_for_verifying(snapshot);
        let witnesses = payload.witnesses();
        if hashes.len() != witnesses.len() {
            return Err(CoreError::other("witness count mismatch"));
        }
        let mut remaining_gas = 6_000_000i64;
        for (hash, witness) in hashes.iter().zip(witnesses) {
            match neo_execution::Helper::verify_witness_with_native_provider(
                payload,
                settings,
                snapshot,
                hash,
                witness,
                remaining_gas,
                native_contract_provider.clone(),
            ) {
                Ok(fee) => remaining_gas -= fee,
                Err(error) => {
                    return Err(CoreError::other(format!(
                        "witness verification failed: {error}"
                    )));
                }
            }
        }
        Ok(())
    }
}
