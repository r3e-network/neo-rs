//! Concrete [`neo_runtime::Nep17MetadataReader`] implementation backed by
//! [`ApplicationEngine`].
//!
//! This is the execution-layer impl of the trait defined in `neo-runtime`.
//! It builds a script that calls `decimals` then `symbol` on the target
//! NEP-17 contract (both with `CallFlags::READ_ONLY`) and runs it through
//! a read-only `ApplicationEngine` with a 0.3 GAS budget — matching C#
//! `Neo.Wallets.AssetDescriptor` semantics.
//!
//! The wallet layer (`neo-wallets`) depends on the trait, not on this
//! concrete impl, breaking the direct L4 → L3 execution dependency.

use std::sync::Arc;

use neo_config::ProtocolSettings;
use neo_primitives::{CallFlags, TriggerType, UInt160};
use neo_runtime::{Nep17Metadata, Nep17MetadataReader, ServiceError};
use neo_storage::DataCache;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::VmState;
use num_traits::ToPrimitive;

use crate::ApplicationEngine;
use crate::native_contract_provider::NativeContractProvider;

/// GAS budget for the metadata probe, matching C# `ApplicationEngine.Run`
/// (`gas: 0_30000000L` — 0.3 GAS).
const DESCRIPTOR_PROBE_GAS: i64 = 30_000_000;

/// [`neo_runtime::Nep17MetadataReader`] backed by [`ApplicationEngine`].
///
/// Holds the snapshot and protocol settings needed to construct an
/// `ApplicationEngine` for each `read_metadata` call. Cheap to clone because
/// the snapshot and native provider are `Arc`s and settings are cloneable.
#[derive(Clone)]
pub struct Nep17MetadataReaderImpl {
    snapshot: Arc<DataCache>,
    settings: ProtocolSettings,
    native_contract_provider: Arc<dyn NativeContractProvider>,
}

impl std::fmt::Debug for Nep17MetadataReaderImpl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Nep17MetadataReaderImpl")
            .field("snapshot", &"DataCache")
            .field("settings", &self.settings)
            .field("native_contract_provider", &"NativeContractProvider")
            .finish()
    }
}

impl Nep17MetadataReaderImpl {
    /// Construct a new reader with an explicit native-contract provider.
    ///
    /// The provider is captured by the reader and passed into each probe engine,
    /// matching the rest of the execution-layer provider pattern: a replay, RPC
    /// server, or embedded node should not observe ambient provider changes made
    /// by unrelated tests or nodes.
    pub fn new_with_native_contract_provider(
        snapshot: Arc<DataCache>,
        settings: ProtocolSettings,
        native_contract_provider: Arc<dyn NativeContractProvider>,
    ) -> Self {
        Self {
            snapshot,
            settings,
            native_contract_provider,
        }
    }
}

impl Nep17MetadataReader for Nep17MetadataReaderImpl {
    fn read_metadata(&self, contract_hash: UInt160) -> Result<Nep17Metadata, ServiceError> {
        let mut builder = ScriptBuilder::new();
        emit_descriptor_call(&mut builder, &contract_hash, "decimals")
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        emit_descriptor_call(&mut builder, &contract_hash, "symbol")
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let script = builder.to_array();

        let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::clone(&self.snapshot),
            None,
            self.settings.clone(),
            DESCRIPTOR_PROBE_GAS,
            None,
            Some(Arc::clone(&self.native_contract_provider)),
        )
        .map_err(|e| ServiceError::Internal(e.to_string()))?;
        engine
            .load_script(script, CallFlags::READ_ONLY, None)
            .map_err(|e| ServiceError::Internal(e.to_string()))?;

        let state = engine.execute_allow_fault();
        if state != VmState::HALT {
            return Err(ServiceError::InvalidInput(format!(
                "Failed to execute 'decimals' or 'symbol' method for asset {contract_hash}. The \
                 contract execution did not complete successfully (VM state: {state:?})."
            )));
        }

        // The script emitted `decimals` first then `symbol`, so the result stack
        // is `[decimals, symbol]` bottom-to-top: `symbol` is on top (index 0),
        // `decimals` below it (index 1) — the same pop order as C#.
        let result_stack = engine.result_stack();

        let symbol_bytes = result_stack
            .peek(0)
            .map_err(|e| ServiceError::Internal(e.to_string()))?
            .as_bytes()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let symbol = String::from_utf8(symbol_bytes).map_err(|e| {
            ServiceError::InvalidInput(format!("asset symbol is not valid UTF-8: {e}"))
        })?;

        let decimals_int = result_stack
            .peek(1)
            .map_err(|e| ServiceError::Internal(e.to_string()))?
            .as_int()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        // C# narrows with `(byte)`; an out-of-range value would overflow there,
        // so reject it here rather than silently wrapping.
        let decimals = decimals_int.to_u8().ok_or_else(|| {
            ServiceError::InvalidInput(format!(
                "asset {contract_hash} reported an out-of-range decimals value: {decimals_int}"
            ))
        })?;

        Ok(Nep17Metadata { symbol, decimals })
    }
}

/// Emits a no-argument dynamic call to `method` on `asset_id` with
/// `CallFlags.ReadOnly`, mirroring C# `ScriptBuilderExtensions.EmitDynamicCall`:
/// push an empty argument array (`PUSH0; PACK`), then the call flags, method
/// name, and contract hash, followed by the `System.Contract.Call` syscall.
fn emit_descriptor_call(
    builder: &mut ScriptBuilder,
    asset_id: &UInt160,
    method: &str,
) -> Result<(), neo_error::CoreError> {
    builder.emit_push_int(0);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::READ_ONLY.bits()));
    builder.emit_push(method.as_bytes());
    builder.emit_push(&asset_id.to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|e| neo_error::CoreError::invalid_operation(e.to_string()))?;
    Ok(())
}
