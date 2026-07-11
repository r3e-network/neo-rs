//! Read-only engine execution for native-contract probes.
//!
//! Query methods decide which native method to call. This module owns the VM
//! setup and HALT validation needed to execute that read against a fixed
//! storage snapshot.

use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_execution::ApplicationEngine;
use neo_manifest::CallFlags;
use neo_primitives::{TriggerType, UInt160};
use neo_storage::persistence::{CacheRead, DataCache};
use neo_vm::StackItem;
use neo_vm_rs::VmState as VMState;

use crate::server::rpc_server::RpcServer;

use super::script::{NativeArg, build_native_call_script};

/// Runs a single read-only native-method call against `snapshot` and returns
/// the top of the result stack.
///
/// Faults are surfaced as errors because the native reads probed here cannot
/// fault on healthy state.
pub(super) fn invoke_native_read<B: CacheRead>(
    server: &RpcServer,
    snapshot: Arc<DataCache<B>>,
    contract: &UInt160,
    method: &str,
    args: &[NativeArg<'_>],
) -> CoreResult<StackItem> {
    let script = build_native_call_script(contract, method, args)?;

    let system = server.system();
    let settings = system.settings().as_ref().clone();
    let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
        TriggerType::Application,
        None,
        snapshot,
        None,
        settings,
        server.settings().max_gas_invoke,
        neo_execution::NoDiagnostic,
        system.native_contract_provider(),
    )
    .map_err(|err| CoreError::other(err.to_string()))?;
    engine
        .load_script(script, CallFlags::READ_ONLY, None)
        .map_err(|err| CoreError::other(err.to_string()))?;
    let state = engine.execute_allow_fault();
    if state != VMState::HALT {
        return Err(CoreError::other(format!(
            "native read '{method}' did not HALT (VM state: {state:?})"
        )));
    }
    engine
        .result_stack()
        .peek(0)
        .cloned()
        .map_err(|err| CoreError::other(err.to_string()))
}
