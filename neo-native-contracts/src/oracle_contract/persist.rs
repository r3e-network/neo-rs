//! Oracle post-persist response cleanup and reward minting.
//!
//! Keeps block-hook orchestration out of the Oracle contract root while
//! preserving C# request removal, per-url id-list maintenance, designated
//! Oracle role selection, and GAS reward semantics.

use super::OracleContract;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, Contract};
use neo_primitives::UInt160;
use neo_storage::StorageItem;
use num_bigint::BigInt;

impl OracleContract {
    /// C# `OracleContract.PostPersistAsync`: for every oracle-response
    /// transaction in the persisting block, remove the answered request
    /// record and its id from the per-url id-list, then mint the oracle
    /// price to the designated oracle node selected by `id % nodes.len()`.
    pub(super) fn post_persist_native<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &mut ApplicationEngine<P, D, B>,
    ) -> CoreResult<()> {
        let (block_index, response_ids): (u32, Vec<u64>) = {
            let block = crate::support::engine::require_persisting_block(
                engine,
                "OracleContract::post_persist",
            )?;
            let ids = block
                .transactions
                .iter()
                .filter_map(|tx| Self::oracle_response_attribute(tx).map(|response| response.id))
                .collect();
            (block.index(), ids)
        };

        let snapshot = engine.snapshot_cache();
        let mut nodes: Option<Vec<(UInt160, BigInt)>> = None;
        for id in response_ids {
            // Remove the request from storage (skip responses without one).
            let key = Self::request_key(id);
            let Some(item) = snapshot.get(&key) else {
                continue;
            };
            let request = Self::decode_oracle_request(&item.value_bytes())?;
            snapshot.delete(&key);

            // Remove the id from the url id-list; C# throws when the id is
            // not listed, and deletes the entry once the list is empty.
            let list_key = Self::id_list_key(&request.url);
            let mut list = match snapshot.get(&list_key) {
                Some(list_item) => Self::decode_id_list(&list_item.value_bytes())?,
                None => Vec::new(),
            };
            let Some(position) = list.iter().position(|listed| *listed == id) else {
                return Err(CoreError::invalid_operation(
                    "OracleContract::post_persist: request id missing from the url id-list",
                ));
            };
            list.remove(position);
            if list.is_empty() {
                snapshot.delete(&list_key);
            } else {
                snapshot.update(
                    list_key,
                    StorageItem::from_bytes(Self::encode_id_list(&list)?),
                );
            }

            // Accumulate the oracle fee for the node selected by the id.
            if nodes.is_none() {
                let points = crate::RoleManagement::new().get_designated_by_role_at(
                    &snapshot,
                    crate::Role::Oracle,
                    block_index,
                )?;
                nodes = Some(
                    points
                        .into_iter()
                        .map(|point| {
                            (
                                UInt160::from_script(&Contract::create_signature_redeem_script(
                                    point,
                                )),
                                BigInt::from(0),
                            )
                        })
                        .collect(),
                );
            }
            if let Some(nodes) = nodes.as_mut() {
                if !nodes.is_empty() {
                    let index = usize::try_from(id % nodes.len() as u64).unwrap_or(0);
                    let price = self.read_price(&snapshot)?;
                    nodes[index].1 += BigInt::from(price);
                }
            }
        }

        if let Some(nodes) = nodes {
            for (account, gas) in nodes {
                if gas > BigInt::from(0) {
                    crate::GasToken::new().gas_mint(engine, &account, &gas, false)?;
                }
            }
        }
        Ok(())
    }
}
