//! Engine-script probes for native-contract reads.
//!
//! The C# RPC server reads governance / token state through direct
//! `NativeContract.NEO.*` accessors. The Rust native-contract crate
//! exposes those reads through the contract ABI (`invoke`) rather than
//! as snapshot helper methods, so this module runs the same native
//! methods through a read-only [`ApplicationEngine`] script — the exact
//! pattern `neo_wallets::AssetDescriptor` uses for its `decimals` /
//! `symbol` probes. Because the probes execute the real native
//! implementations, the results are byte-identical to an on-chain
//! invocation.

use std::sync::Arc;

use neo_execution::ApplicationEngine;
use neo_manifest::CallFlags;
use neo_primitives::{TriggerType, UInt160};
use neo_vm::script_builder::ScriptBuilder;
use neo_storage::persistence::DataCache;
use neo_vm::StackItem;
use neo_vm_rs::VmState as VMState;
use num_bigint::BigInt;

use crate::server::rpc_server::RpcServer;

/// Builds a [`neo_execution::NativeRegistry`] populated with the
/// standard native contracts. `NativeRegistry::new()` is *empty* by
/// design; the canonical contract set lives in
/// [`neo_native_contracts::StandardNativeProvider`].
pub(crate) fn native_registry() -> neo_execution::NativeRegistry {
    use neo_execution::native_contract_provider::NativeContractProvider;
    let mut registry = neo_execution::NativeRegistry::new();
    for contract in neo_native_contracts::StandardNativeProvider::new().all_native_contracts() {
        registry.register(contract);
    }
    registry
}

/// Argument value for a native-contract probe call.
pub(crate) enum NativeArg<'a> {
    /// Raw byte-string argument (hashes, public keys, …).
    Bytes(&'a [u8]),
    /// Integer argument.
    Int(i64),
}

/// Emits a dynamic call to `method` on `contract` with the given
/// arguments and `CallFlags::READ_ONLY`, mirroring C#
/// `ScriptBuilderExtensions.EmitDynamicCall`: push the argument array
/// (reversed, then `PACK`), then call flags, method name, contract
/// hash, and the `System.Contract.Call` syscall.
fn emit_native_call(
    builder: &mut ScriptBuilder,
    contract: &UInt160,
    method: &str,
    args: &[NativeArg<'_>],
) -> Result<(), String> {
    if args.is_empty() {
        builder.emit_push_int(0);
        builder.emit_pack();
    } else {
        for arg in args.iter().rev() {
            match arg {
                NativeArg::Bytes(bytes) => {
                    builder.emit_push(bytes);
                }
                NativeArg::Int(value) => {
                    builder.emit_push_int(*value);
                }
            }
        }
        builder.emit_push_int(args.len() as i64);
        builder.emit_pack();
    }
    builder.emit_push_int(i64::from(CallFlags::READ_ONLY.bits()));
    builder.emit_push(method.as_bytes());
    builder.emit_push(&contract.to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .map_err(|err| err.to_string())?;
    Ok(())
}

/// Runs a single read-only native-method call against `snapshot` and
/// returns the top of the result stack. Faults are surfaced as errors
/// (the native reads probed here cannot fault on healthy state).
pub(crate) fn invoke_native_read(
    server: &RpcServer,
    snapshot: Arc<DataCache>,
    contract: &UInt160,
    method: &str,
    args: &[NativeArg<'_>],
) -> Result<StackItem, String> {
    let mut builder = ScriptBuilder::new();
    emit_native_call(&mut builder, contract, method, args)?;
    let script = builder.to_array();

    let settings = server.system().settings().as_ref().clone();
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        settings,
        server.settings().max_gas_invoke,
        None,
    )
    .map_err(|err| err.to_string())?;
    engine
        .load_script(script, CallFlags::READ_ONLY, None)
        .map_err(|err| err.to_string())?;
    let state = engine.execute_allow_fault();
    if state != VMState::HALT {
        return Err(format!(
            "native read '{method}' did not HALT (VM state: {state:?})"
        ));
    }
    engine
        .result_stack()
        .peek(0)
        .map(StackItem::clone)
        .map_err(|err| err.to_string())
}

/// `NEO.unclaimedGas(account, end)` — the amount of unclaimed GAS for
/// `account` at the `end` block height.
pub(crate) fn neo_unclaimed_gas(
    server: &RpcServer,
    snapshot: Arc<DataCache>,
    neo_hash: &UInt160,
    account: &UInt160,
    end: u32,
) -> Result<BigInt, String> {
    let account_bytes = account.to_bytes();
    let item = invoke_native_read(
        server,
        snapshot,
        neo_hash,
        "unclaimedGas",
        &[
            NativeArg::Bytes(account_bytes.as_slice()),
            NativeArg::Int(i64::from(end)),
        ],
    )?;
    item.as_int().map_err(|err| err.to_string())
}

/// `NEO.getCommittee()` — the current committee public keys (sorted).
pub(crate) fn neo_committee(
    server: &RpcServer,
    snapshot: Arc<DataCache>,
    neo_hash: &UInt160,
) -> Result<Vec<Vec<u8>>, String> {
    let item = invoke_native_read(server, snapshot, neo_hash, "getCommittee", &[])?;
    stack_array_of_bytes(&item)
}

/// `NEO.getNextBlockValidators()` — the validators for the next block.
pub(crate) fn neo_next_block_validators(
    server: &RpcServer,
    snapshot: Arc<DataCache>,
    neo_hash: &UInt160,
) -> Result<Vec<Vec<u8>>, String> {
    let item = invoke_native_read(server, snapshot, neo_hash, "getNextBlockValidators", &[])?;
    stack_array_of_bytes(&item)
}

/// `NEO.getCandidates()` — registered candidates with their votes.
pub(crate) fn neo_candidates(
    server: &RpcServer,
    snapshot: Arc<DataCache>,
    neo_hash: &UInt160,
) -> Result<Vec<(Vec<u8>, BigInt)>, String> {
    let item = invoke_native_read(server, snapshot, neo_hash, "getCandidates", &[])?;
    let entries = item.as_array().map_err(|err| err.to_string())?;
    let mut candidates = Vec::with_capacity(entries.len());
    for entry in entries {
        let fields = entry.as_array().map_err(|err| err.to_string())?;
        if fields.len() != 2 {
            return Err(format!(
                "getCandidates entry has {} fields, expected 2",
                fields.len()
            ));
        }
        let pubkey = fields[0].as_bytes().map_err(|err| err.to_string())?;
        let votes = fields[1].as_int().map_err(|err| err.to_string())?;
        candidates.push((pubkey, votes));
    }
    Ok(candidates)
}

/// `Policy.isBlocked(account)` — whether the account is on the
/// `PolicyContract` block list.
pub(crate) fn policy_is_blocked(
    server: &RpcServer,
    snapshot: Arc<DataCache>,
    policy_hash: &UInt160,
    account: &UInt160,
) -> Result<bool, String> {
    let account_bytes = account.to_bytes();
    let item = invoke_native_read(
        server,
        snapshot,
        policy_hash,
        "isBlocked",
        &[NativeArg::Bytes(account_bytes.as_slice())],
    )?;
    item.as_bool().map_err(|err| err.to_string())
}

/// `NEO.getCandidateVote(pubkey)` — the candidate's vote count, or `-1`
/// when the key is not a registered candidate.
pub(crate) fn neo_candidate_vote(
    server: &RpcServer,
    snapshot: Arc<DataCache>,
    neo_hash: &UInt160,
    pubkey: &[u8],
) -> Result<BigInt, String> {
    let item = invoke_native_read(
        server,
        snapshot,
        neo_hash,
        "getCandidateVote",
        &[NativeArg::Bytes(pubkey)],
    )?;
    item.as_int().map_err(|err| err.to_string())
}

/// Decodes a stack array whose elements are byte strings.
fn stack_array_of_bytes(item: &StackItem) -> Result<Vec<Vec<u8>>, String> {
    let entries = item.as_array().map_err(|err| err.to_string())?;
    entries
        .iter()
        .map(|entry| entry.as_bytes().map_err(|err| err.to_string()))
        .collect()
}
