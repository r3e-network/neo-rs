//! ContractManagement call guards, payload parsing, and deployment validation.

use super::super::ContractManagement;
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::{ApplicationEngine, ContractState};
use neo_manifest::manifest::contract_manifest::MAX_MANIFEST_LENGTH;
use neo_manifest::{ContractAbi, ContractManifest, NefFile};
use neo_primitives::{CallFlags, UInt160};
use neo_serialization::BinarySerializer;
use neo_vm::ExecutionEngineLimits;
use neo_vm::StackItem;
use std::collections::HashSet;

impl ContractManagement {
    /// C# Deploy/Update post-Aspidochelone guard (refs neo#2653 / neo#2673): the
    /// current (native) context must carry `CallFlags.All`, i.e. the caller must
    /// have requested a full-trust call.
    pub(in crate::contract_management) fn require_call_flags_all<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &ApplicationEngine<P, D, B>,
        method: &str,
    ) -> CoreResult<()> {
        if !engine.is_hardfork_enabled(Hardfork::HfAspidochelone) {
            return Ok(());
        }
        let flags = engine.get_current_call_flags().map_err(|e| {
            CoreError::invalid_operation(format!("ContractManagement::{method}: call flags: {e}"))
        })?;
        if !flags.contains(CallFlags::ALL) {
            return Err(CoreError::invalid_operation(format!(
                "Cannot call {method} with the flag {flags:?}."
            )));
        }
        Ok(())
    }

    /// Returns whether native-call argument `index` was pushed as `StackItem::Null`
    /// (bit `index` of the dispatcher's null mask). This is the only
    /// reliable null signal: a `Null` ByteArray arg reaches the `Vec<u8>` layer as
    /// the 1-byte serialized-null payload, not as empty bytes.
    pub(in crate::contract_management) fn native_arg_is_null<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &ApplicationEngine<P, D, B>,
        index: usize,
    ) -> bool {
        engine.native_arg_is_null(index)
    }

    /// C# `nefFile.AsSerializable<NefFile>()` with the preceding
    /// `nefFile.Length == 0` guard: rejects empty payloads, then parses the NEF3
    /// container (magic + checksum validation included in `NefFile::deserialize`).
    pub(in crate::contract_management) fn parse_nef_checked(
        bytes: &[u8],
        method: &str,
    ) -> CoreResult<NefFile> {
        if bytes.is_empty() {
            return Err(CoreError::invalid_operation(format!(
                "ContractManagement::{method}: NEF file length cannot be zero"
            )));
        }
        NefFile::parse(bytes).map_err(|e| {
            CoreError::invalid_operation(format!("ContractManagement::{method}: bad NEF: {e}"))
        })
    }

    /// C# `ContractManifest.Parse(manifest)` with the preceding
    /// `manifest.Length == 0` guard: the byte-length cap from
    /// `Parse(ReadOnlySpan<byte>)` (`MaxLength` = u16::MAX), JSON parsing, and the
    /// `FromJson` structural checks (non-empty name, empty features, unique
    /// groups / standards / permissions / trusts) which `validate()` mirrors.
    pub(in crate::contract_management) fn parse_manifest_checked(
        bytes: &[u8],
        method: &str,
    ) -> CoreResult<ContractManifest> {
        if bytes.is_empty() {
            return Err(CoreError::invalid_operation(format!(
                "ContractManagement::{method}: manifest length cannot be zero"
            )));
        }
        if bytes.len() > MAX_MANIFEST_LENGTH {
            return Err(CoreError::invalid_operation(format!(
                "ContractManagement::{method}: manifest length {} exceeds maximum {}",
                bytes.len(),
                MAX_MANIFEST_LENGTH
            )));
        }
        let json = std::str::from_utf8(bytes).map_err(|e| {
            CoreError::invalid_operation(format!(
                "ContractManagement::{method}: manifest is not UTF-8: {e}"
            ))
        })?;
        let manifest = ContractManifest::parse(json).map_err(|e| {
            CoreError::invalid_operation(format!("ContractManagement::{method}: bad manifest: {e}"))
        })?;
        manifest.validate().map_err(|e| {
            CoreError::invalid_operation(format!("ContractManagement::{method}: bad manifest: {e}"))
        })?;
        Ok(manifest)
    }

    /// C# `Helper.Check(new Script(script, strict), abi)`:
    /// - strict (post-Basilisk): full structural script validation, and every ABI
    ///   method offset must land on a parsed instruction boundary
    ///   (`Script.GetInstruction` throws for non-boundary offsets in strict mode);
    /// - non-strict: each offset must be in range and the instruction at that
    ///   exact offset must parse (`Instruction.Parse` on demand);
    /// - both: method `(name, pcount)` pairs and event names must be unique
    ///   (C# `abi.GetMethod("", 0)` dictionary construction + events
    ///   `ToDictionary`).
    pub(in crate::contract_management) fn check_script_against_abi(
        script: &[u8],
        abi: &ContractAbi,
        strict: bool,
    ) -> CoreResult<()> {
        let validated = if strict {
            Some(neo_vm::validate_script(script, true).map_err(|e| {
                CoreError::invalid_operation(format!("ContractManagement: invalid script: {e}"))
            })?)
        } else {
            None
        };
        for method in &abi.methods {
            let offset = usize::try_from(method.offset).map_err(|_| {
                CoreError::invalid_operation(format!(
                    "ContractManagement: method '{}' has a negative offset",
                    method.name
                ))
            })?;
            if offset >= script.len() {
                return Err(CoreError::invalid_operation(format!(
                    "ContractManagement: method '{}' offset {} is out of script range {}",
                    method.name,
                    offset,
                    script.len()
                )));
            }
            match &validated {
                Some(validated) => {
                    if !validated.has_instruction_at(offset) {
                        return Err(CoreError::invalid_operation(format!(
                            "ContractManagement: method '{}' offset {} is not an instruction boundary",
                            method.name, offset
                        )));
                    }
                }
                None => {
                    neo_vm::Instruction::parse(script, offset).map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement: method '{}' offset {}: {e}",
                            method.name, offset
                        ))
                    })?;
                }
            }
        }
        let mut method_keys = HashSet::new();
        for method in &abi.methods {
            if !method_keys.insert((method.name.as_str(), method.parameters.len())) {
                return Err(CoreError::invalid_operation(format!(
                    "ContractManagement: duplicate ABI method '{}' with {} parameter(s)",
                    method.name,
                    method.parameters.len()
                )));
            }
        }
        let mut event_names = HashSet::new();
        for event in &abi.events {
            if !event_names.insert(event.name.as_str()) {
                return Err(CoreError::invalid_operation(format!(
                    "ContractManagement: duplicate ABI event '{}'",
                    event.name
                )));
            }
        }
        Ok(())
    }

    /// C# `ContractManifest.IsValid(limits, hash)`: the manifest must serialize as
    /// a stack item within the engine limits, and every group signature must
    /// verify (secp256r1) against the contract hash.
    pub(in crate::contract_management) fn manifest_is_valid(
        manifest: &ContractManifest,
        limits: &ExecutionEngineLimits,
        hash: &UInt160,
    ) -> bool {
        if BinarySerializer::serialize(&manifest.to_stack_item(), limits).is_err() {
            return false;
        }
        manifest
            .groups
            .iter()
            .all(|group| group.verify_signature(&hash.to_bytes()).unwrap_or(false))
    }

    /// Serializes a `ContractState` into the per-contract record bytes — the C#
    /// interoperable form (`BinarySerializer.Serialize(state.ToStackItem(null))`,
    /// see `StorageItem.Value` over `IInteroperable`), the encoding that
    /// `get_contract_from_snapshot` reads.
    pub(in crate::contract_management) fn serialize_contract_record(
        state: &ContractState,
    ) -> CoreResult<Vec<u8>> {
        state.serialize_contract_record()
    }

    /// Decodes the optional trailing `data: Any` argument shared by the 3-arg
    /// `deploy` / `update` overloads. The 2-arg overloads and an explicit `Null`
    /// argument both yield `StackItem::Null` (C# passes `StackItem.Null` through).
    pub(in crate::contract_management) fn optional_data_arg<
        P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    >(
        &self,
        engine: &ApplicationEngine<P, D, B>,
        args: &[Vec<u8>],
        method: &str,
    ) -> CoreResult<StackItem> {
        if args.len() < 3 || self.native_arg_is_null(engine, 2) {
            return Ok(StackItem::Null);
        }
        let limits = *engine.execution_limits();
        BinarySerializer::deserialize(&args[2], &limits, None).map_err(|e| {
            CoreError::invalid_operation(format!("ContractManagement::{method}: bad data arg: {e}"))
        })
    }
}
