use neo_config::ProtocolSettings;
use neo_error::{CoreError, CoreResult};
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_execution::{ApplicationEngine, NativeContract};
use neo_payloads::Block;
use neo_primitives::{TriggerType, UInt256};

use super::LEDGER_CONTRACT_ID;

/// Runs the per-block native hook matching `engine`'s trigger
/// (`on_persist` for [`TriggerType::OnPersist`], `post_persist` for
/// [`TriggerType::PostPersist`]) for every contract in `contracts` that
/// is active at `block_index`, in the given canonical registration order.
///
/// This is the exact body of C#'s `System.Contract.NativeOnPersist` /
/// `NativePostPersist` syscalls (`NativeContract.OnPersistAsync` /
/// `PostPersistAsync` over `Contracts.Where(IsActive)`). A hook error aborts
/// the block, like the C# native script faulting.
pub(super) fn run_native_persist_hooks<P, B>(
    contracts: &[P::Contract],
    engine: &mut ApplicationEngine<P, neo_execution::NoDiagnostic, B>,
    settings: &ProtocolSettings,
    block: &Block,
    block_hash: &UInt256,
    block_index: u32,
) -> CoreResult<()>
where
    P: NativeContractProvider + 'static,
    B: neo_storage::CacheRead,
{
    let trigger = engine.trigger_type();
    let metric_hook = match trigger {
        TriggerType::OnPersist => neo_runtime::sync_metrics::NativePersistHook::OnPersist,
        TriggerType::PostPersist => neo_runtime::sync_metrics::NativePersistHook::PostPersist,
        other => {
            return Err(CoreError::invalid_operation(format!(
                "native persist hooks require an OnPersist/PostPersist engine, got {other:?}"
            )));
        }
    };
    for contract in contracts {
        if !contract.is_active(settings, block_index) {
            continue;
        }
        let hook_start = std::time::Instant::now();
        if contract.id() == LEDGER_CONTRACT_ID {
            let snapshot = engine.snapshot_cache();
            match trigger {
                TriggerType::OnPersist => {
                    crate::ledger_records::LedgerRecords::write_on_persist_records(
                        &snapshot, block, block_hash,
                    )?;
                }
                TriggerType::PostPersist => {
                    crate::ledger_records::LedgerRecords::write_post_persist_record(
                        &snapshot,
                        block_hash,
                        block_index,
                    )?;
                }
                _ => {}
            }
        }
        let result = match metric_hook {
            neo_runtime::sync_metrics::NativePersistHook::OnPersist => contract.on_persist(engine),
            neo_runtime::sync_metrics::NativePersistHook::PostPersist => {
                contract.post_persist(engine)
            }
        };
        neo_runtime::sync_metrics::record_native_contract_hook(
            metric_hook,
            contract.id(),
            neo_runtime::time::elapsed_us(hook_start.elapsed()),
        );
        result.map_err(|e| {
            CoreError::invalid_operation(format!(
                "native {} {trigger:?} hook failed at block {block_index}: {e}",
                contract.name()
            ))
        })?;
    }
    Ok(())
}
