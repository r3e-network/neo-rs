//! ContractManagement native contract.
//!
//! Concrete implementation of the ContractManagement native contract:
//! the read-side surface (look up a deployed contract by hash / id),
//! the `deploy` / `update` writers (NEF and manifest validation, the
//! record and id-index writes, the `_deploy` callback and the
//! Deploy/Update events), and the `destroy` writer. The read surface
//! is consumed by oracle service, RPC, the application engine, and
//! the tokens tracker, so it lives here so all those crates can share
//! it without depending on `neo-blockchain`.

use crate::hashes::CONTRACT_MANAGEMENT_HASH;
use neo_config::Hardfork;
use neo_error::{CoreError, CoreResult};
use neo_execution::application_engine_contract::NativeArgNullMask;
use neo_execution::helper::Helper;
use neo_execution::{
    ApplicationEngine, ContractState, Interoperable, NativeContract, NativeEvent, NativeMethod,
};
use neo_io::{BinaryWriter, MemoryReader, Serializable};
use neo_manifest::manifest::contract_manifest::MAX_MANIFEST_LENGTH;
use neo_manifest::{ContractAbi, ContractManifest, NefFile};
use neo_payloads::transaction::Transaction;
use neo_primitives::{CallFlags, ContractBasicMethod, ContractParameterType, FindOptions, UInt160};
use neo_serialization::BinarySerializer;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::{ExecutionEngineLimits, StackValue};
use num_bigint::BigInt;
use num_traits::ToPrimitive;
use std::any::Any;
use std::collections::HashSet;
use std::sync::LazyLock;

/// Storage prefix for the minimum-deployment-fee setting (C#
/// `ContractManagement.Prefix_MinimumDeploymentFee`).
const PREFIX_MINIMUM_DEPLOYMENT_FEE: u8 = 20;
/// C# default minimum deployment fee: 10 GAS, in datoshi.
const DEFAULT_MINIMUM_DEPLOYMENT_FEE: i64 = 10_00000000;

/// Storage prefix for the per-contract record (matches C#
/// `ContractManagement.PREFIX_CONTRACT`).
const PREFIX_CONTRACT: u8 = 8;
/// Storage prefix for the contract-id → hash index (matches C#
/// `ContractManagement.PREFIX_CONTRACT_HASH`).
const PREFIX_CONTRACT_HASH: u8 = 12;
/// Storage prefix for the next-available-contract-id counter (matches C#
/// `ContractManagement.Prefix_NextAvailableId`).
const PREFIX_NEXT_AVAILABLE_ID: u8 = 15;
/// C# genesis value for `Prefix_NextAvailableId` (`InitializeAsync` writes 1).
const DEFAULT_NEXT_AVAILABLE_ID: i64 = 1;

/// C# `PolicyContract.Prefix_BlockedAccount` — written cross-natively here by
/// `destroy` (C# `ContractManagement.Destroy` → `Policy.BlockAccountInternal`).
const POLICY_PREFIX_BLOCKED_ACCOUNT: u8 = 15;
/// C# `PolicyContract.Prefix_WhitelistedFeeContracts` — cleaned cross-natively
/// here by `destroy` (C# `ContractManagement.Destroy` → `Policy.CleanWhitelist`).
const POLICY_PREFIX_WHITELISTED_FEE_CONTRACTS: u8 = 16;

/// Static accessor for the ContractManagement native contract.
#[derive(Debug, Default, Clone, Copy)]
pub struct ContractManagement;

impl ContractManagement {
    /// Stable native contract id (matches C# `ContractManagement.Id`).
    pub const ID: i32 = -1;
    /// Stable native contract name (matches C# `ContractManagement.Name`).
    pub const NAME: &'static str = "ContractManagement";

    /// Constructs a new `ContractManagement` handle.
    pub fn new() -> Self {
        Self
    }

    /// Returns the script hash of the ContractManagement contract.
    pub fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    /// Returns the script hash of the ContractManagement contract (static).
    pub fn script_hash() -> UInt160 {
        *CONTRACT_MANAGEMENT_HASH
    }

    /// Looks up a deployed contract by its script hash.
    ///
    /// Reads the per-contract record (`prefix 8` + `hash.to_bytes()`)
    /// previously written by `ContractManagement.Deploy` /
    /// `ContractManagement.Update` in the blockchain service.
    pub fn get_contract_from_snapshot(
        snapshot: &DataCache,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        let key = Self::contract_storage_key(hash);
        let Some(item) = snapshot.get(&key) else {
            return Ok(None);
        };

        let bytes = item.value_bytes().into_owned();
        if bytes.is_empty() {
            return Ok(None);
        }

        let state = ContractState::deserialize_contract_record(&bytes).map_err(|e| {
            CoreError::deserialization(format!("Failed to deserialize contract state: {e}"))
        })?;
        Ok(Some(state))
    }

    /// Looks up a deployed contract by its contract id.
    ///
    /// Reads the contract-id → hash index (`prefix 12` + `id_be_bytes`)
    /// then dereferences the resulting hash via `get_contract_from_snapshot`.
    pub fn get_contract_by_id_from_snapshot(
        snapshot: &DataCache,
        id: i32,
    ) -> CoreResult<Option<ContractState>> {
        let id_key = Self::contract_id_storage_key(id);
        let Some(item) = snapshot.get(&id_key) else {
            return Ok(None);
        };
        let hash_bytes = item.value_bytes().into_owned();

        if hash_bytes.is_empty() {
            return Ok(None);
        }

        let hash = UInt160::from_bytes(&hash_bytes)
            .map_err(|e| CoreError::invalid_data(format!("Invalid contract hash bytes: {e}")))?;
        Self::get_contract_from_snapshot(snapshot, &hash)
    }

    /// Checks whether a contract is deployed in the given snapshot.
    pub fn is_contract(snapshot: &DataCache, hash: &UInt160) -> bool {
        let key = Self::contract_storage_key(hash);
        snapshot.get(&key).is_some()
    }

    #[inline]
    fn contract_storage_key(hash: &UInt160) -> StorageKey {
        StorageKey::create_with_uint160(Self::ID, PREFIX_CONTRACT, hash)
    }

    #[inline]
    fn contract_id_storage_key(id: i32) -> StorageKey {
        StorageKey::create_with_int32(Self::ID, PREFIX_CONTRACT_HASH, id)
    }

    /// Parses the leading `Hash160` argument shared by `getContract`/`isContract`.
    fn parse_hash_arg(args: &[Vec<u8>], method: &str) -> CoreResult<UInt160> {
        crate::args::raw_account(args, &format!("ContractManagement::{method}"))
    }

    /// C# `ContractAbi.GetMethod(name, pcount) != null`: true when the manifest ABI
    /// declares a method named `name` whose parameter count matches `pcount`, where
    /// `pcount == -1` matches any count.
    fn abi_has_method(manifest: &neo_manifest::ContractManifest, name: &str, pcount: i32) -> bool {
        manifest
            .abi
            .methods
            .iter()
            .any(|m| m.name == name && (pcount == -1 || m.parameters.len() as i32 == pcount))
    }

    /// Marshals a `ContractState` to the Array return bytes (C# `ToStackItem` +
    /// `BinarySerializer`) via the canonical `StackValue` projection — shared by
    /// `getContract` / `getContractById`. A miss is the caller's responsibility
    /// (an empty payload encodes the C# `null`).
    fn contract_state_to_bytes(state: &ContractState, method: &str) -> CoreResult<Vec<u8>> {
        let item = state.to_stack_value();
        BinarySerializer::serialize_stack_value_default(&item).map_err(|e| {
            CoreError::invalid_operation(format!("ContractManagement::{method}: serialize: {e}"))
        })
    }

    /// Collects the `Prefix_ContractHash` storage entries (`id -> hash`) in
    /// forward-seek order, the backing set for C# `GetContractHashes`'s iterator.
    ///
    /// C# reads the contract id back out of each key
    /// (`ReadInt32BigEndian(key.Key[1..])`) and keeps only `id >= 0`, which
    /// excludes the native contracts (negative ids; their big-endian
    /// two's-complement keys sort after every non-negative id).
    fn contract_hash_entries(&self, snapshot: &DataCache) -> Vec<(StorageKey, StorageItem)> {
        let prefix_key = StorageKey::create(ContractManagement::ID, PREFIX_CONTRACT_HASH);
        snapshot
            .find(Some(&prefix_key), SeekDirection::Forward)
            .filter(|(key, _)| {
                let suffix = key.suffix();
                suffix.len() >= 5
                    && i32::from_be_bytes([suffix[1], suffix[2], suffix[3], suffix[4]]) >= 0
            })
            .collect()
    }

    /// C# `NativeContract.IsNative(hash)`: whether the hash belongs to one of the
    /// 11 registered native contracts.
    fn is_native_contract_hash(hash: &UInt160) -> bool {
        crate::catalog::is_standard_native_contract_hash(hash)
    }

    /// C# `PolicyContract.CleanWhitelist(engine, contract)` (PolicyContract.cs
    /// ~368), invoked cross-natively by `ContractManagement.Destroy`: deletes every
    /// `Prefix_WhitelistedFeeContracts ++ contract.Hash` entry and emits Policy's
    /// `WhitelistFeeChanged` event (`[hash, method, argCount, null]`) per removal.
    /// Entries decode as the C# `WhitelistedContract` interoperable
    /// `Struct[ContractHash, Method, ArgCount, FixedFee]`.
    fn policy_clean_whitelist(
        &self,
        engine: &mut ApplicationEngine,
        contract: &ContractState,
    ) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        let prefix_key = StorageKey::create_with_uint160(
            crate::PolicyContract::ID,
            POLICY_PREFIX_WHITELISTED_FEE_CONTRACTS,
            &contract.hash,
        );
        let entries: Vec<(StorageKey, StorageItem)> = snapshot
            .find(Some(&prefix_key), SeekDirection::Forward)
            .collect();
        for (key, item) in entries {
            snapshot.delete(&key);
            let limits = ExecutionEngineLimits::default();
            let decoded = BinarySerializer::deserialize_stack_value_with_limits(
                &item.value_bytes(),
                limits.max_item_size as usize,
                limits.max_stack_size as usize,
            )
            .map_err(|e| {
                CoreError::invalid_operation(format!(
                    "ContractManagement::destroy: whitelist entry: {e}"
                ))
            })?;
            let StackValue::Struct(_, items) = decoded else {
                return Err(CoreError::invalid_data(
                    "whitelisted-contract entry is not a struct",
                ));
            };
            if items.len() < 4 {
                return Err(CoreError::invalid_data(
                    "whitelisted-contract entry must have 4 fields",
                ));
            }
            let hash_bytes = items[0].to_byte_string_bytes().ok_or_else(|| {
                CoreError::invalid_data("whitelisted-contract hash is not byte-like")
            })?;
            crate::args::bytes_to_hash160(&hash_bytes, "whitelisted contract hash")?;
            let method = items
                .get(1)
                .and_then(neo_vm_rs::stack_value_as_string)
                .ok_or_else(|| CoreError::invalid_data("whitelist method is not UTF-8"))?;
            let arg_count = items
                .get(2)
                .and_then(neo_vm_rs::stack_value_as_i64)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| CoreError::invalid_data("whitelist argCount out of range"))?;
            let _fixed_fee = items
                .get(3)
                .and_then(neo_vm_rs::stack_value_as_i64)
                .ok_or_else(|| CoreError::invalid_data("whitelist fixedFee out of range"))?;
            engine
                .send_notification(
                    crate::PolicyContract::script_hash(),
                    "WhitelistFeeChanged".to_string(),
                    vec![
                        StackItem::from_byte_string(contract.hash.to_bytes()),
                        StackItem::from_byte_string(method.into_bytes()),
                        StackItem::from_int(arg_count),
                        StackItem::Null,
                    ],
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "ContractManagement::destroy: notify: {e}"
                    ))
                })?;
        }
        Ok(())
    }

    fn read_required_i64_setting(
        snapshot: &DataCache,
        prefix: u8,
        setting: &str,
    ) -> CoreResult<i64> {
        let key = StorageKey::create(ContractManagement::ID, prefix);
        let Some(item) = snapshot.get(&key) else {
            return Err(CoreError::invalid_data(format!(
                "ContractManagement {setting} is missing"
            )));
        };
        BigInt::from_signed_bytes_le(&item.value_bytes())
            .to_i64()
            .ok_or_else(|| {
                CoreError::invalid_operation(format!("ContractManagement {setting} out of range"))
            })
    }

    fn read_minimum_deployment_fee(&self, snapshot: &DataCache) -> CoreResult<i64> {
        Self::read_required_i64_setting(
            snapshot,
            PREFIX_MINIMUM_DEPLOYMENT_FEE,
            "MinimumDeploymentFee",
        )
    }

    fn read_next_available_id(&self, snapshot: &DataCache) -> CoreResult<i64> {
        Self::read_required_i64_setting(snapshot, PREFIX_NEXT_AVAILABLE_ID, "NextAvailableId")
    }

    /// C# `SetMinimumDeploymentFee` storage effect: overwrite
    /// `Prefix_MinimumDeploymentFee` (`GetAndChange(...).Set(value)`). The key is
    /// genesis-initialised, so absence faults; the value is stored as the full
    /// signed-LE BigInteger (the C# parameter is `BigInteger`, not `long`).
    fn put_minimum_deployment_fee(&self, snapshot: &DataCache, value: &BigInt) -> CoreResult<()> {
        let key = StorageKey::create(ContractManagement::ID, PREFIX_MINIMUM_DEPLOYMENT_FEE);
        if snapshot.get(&key).is_none() {
            return Err(CoreError::invalid_data(
                "ContractManagement MinimumDeploymentFee is missing",
            ));
        }
        snapshot.update(
            key,
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(value)),
        );
        Ok(())
    }

    /// C# `ContractManagement.GetNextAvailableId`: returns the current
    /// `Prefix_NextAvailableId` value and stores `value + 1`
    /// (`item.Add(1)`). The key is genesis-initialised to 1; absence faults.
    fn get_next_available_id(&self, snapshot: &DataCache) -> CoreResult<i32> {
        let value = self.read_next_available_id(snapshot)?;
        let id = i32::try_from(value).map_err(|_| {
            // C# casts `(int)(BigInteger)item`, which throws on overflow.
            CoreError::invalid_operation("next available contract id out of range")
        })?;
        snapshot.update(
            StorageKey::create(ContractManagement::ID, PREFIX_NEXT_AVAILABLE_ID),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                i64::from(id) + 1,
            ))),
        );
        Ok(id)
    }

    /// C# Deploy/Update post-Aspidochelone guard (refs neo#2653 / neo#2673): the
    /// current (native) context must carry `CallFlags.All`, i.e. the caller must
    /// have requested a full-trust call.
    fn require_call_flags_all(&self, engine: &ApplicationEngine, method: &str) -> CoreResult<()> {
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
    /// (bit `index` of the dispatcher's [`NativeArgNullMask`]). This is the only
    /// reliable null signal: a `Null` ByteArray arg reaches the `Vec<u8>` layer as
    /// the 1-byte serialized-null payload, not as empty bytes.
    fn native_arg_is_null(&self, engine: &ApplicationEngine, index: usize) -> bool {
        index < 32
            && engine
                .get_state::<NativeArgNullMask>()
                .is_some_and(|mask| mask.0 & (1u32 << index) != 0)
    }

    /// C# `nefFile.AsSerializable<NefFile>()` with the preceding
    /// `nefFile.Length == 0` guard: rejects empty payloads, then parses the NEF3
    /// container (magic + checksum validation included in `NefFile::deserialize`).
    fn parse_nef_checked(bytes: &[u8], method: &str) -> CoreResult<NefFile> {
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
    fn parse_manifest_checked(bytes: &[u8], method: &str) -> CoreResult<ContractManifest> {
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
    fn check_script_against_abi(script: &[u8], abi: &ContractAbi, strict: bool) -> CoreResult<()> {
        let validated = if strict {
            Some(neo_vm_rs::validate_script(script, true).map_err(|e| {
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
                    neo_vm_rs::Instruction::parse(script, offset).map_err(|e| {
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
    fn manifest_is_valid(
        manifest: &ContractManifest,
        limits: &ExecutionEngineLimits,
        hash: &UInt160,
    ) -> bool {
        if BinarySerializer::serialize_stack_value(&manifest.to_stack_value(), limits).is_err() {
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
    fn serialize_contract_record(state: &ContractState) -> CoreResult<Vec<u8>> {
        state.serialize_contract_record()
    }

    /// Decodes the optional trailing `data: Any` argument shared by the 3-arg
    /// `deploy` / `update` overloads. The 2-arg overloads and an explicit `Null`
    /// argument both yield `StackItem::Null` (C# passes `StackItem.Null` through).
    fn optional_data_arg(
        &self,
        engine: &ApplicationEngine,
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

    /// C# `ContractManagement.OnDeployAsync`: invoke the contract's `_deploy(data,
    /// update)` callback when (and only when) its manifest ABI declares it with
    /// exactly two parameters, then emit the `Deploy` / `Update` event.
    ///
    /// The callback goes through `queue_contract_call_from_native` (the faithful
    /// equivalent of C# `CallFromNativeContractAsync` in this engine, proven by
    /// the NEP-17 `onNEP17Payment` path): it executes after the native method
    /// returns, against the record this method has already written, and a fault
    /// inside `_deploy` still faults the whole transaction as in C#.
    fn on_deploy(
        &self,
        engine: &mut ApplicationEngine,
        contract: &ContractState,
        data: StackItem,
        update: bool,
    ) -> CoreResult<()> {
        if Self::abi_has_method(
            &contract.manifest,
            ContractBasicMethod::DEPLOY,
            ContractBasicMethod::DEPLOY_P_COUNT,
        ) {
            engine.queue_contract_call_from_native(
                ContractManagement::script_hash(),
                contract.hash,
                ContractBasicMethod::DEPLOY,
                vec![data, StackItem::from_bool(update)],
            );
        }
        let event = if update { "Update" } else { "Deploy" };
        engine
            .send_notification(
                ContractManagement::script_hash(),
                event.to_string(),
                vec![StackItem::from_byte_string(contract.hash.to_bytes())],
            )
            .map_err(|e| {
                CoreError::invalid_operation(format!("ContractManagement: {event} notify: {e}"))
            })
    }

    /// C# `ContractManagement.Deploy(engine, nefFile, manifest, data)` (~239-303):
    /// validates the caller / payloads, charges
    /// `max(StoragePrice * payload, GetMinimumDeploymentFee)`, computes the
    /// contract hash from `(tx.Sender, nef.CheckSum, manifest.Name)`, allocates
    /// the next contract id, writes the record + big-endian id index, runs the
    /// `_deploy` callback, emits `Deploy`, and returns the new `ContractState`.
    fn deploy(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        // Post-Aspidochelone the caller must hold CallFlags.All.
        self.require_call_flags_all(engine, "Deploy")?;
        // C#: `engine.ScriptContainer is not Transaction tx` -> throw; the sender
        // is the transaction's first signer.
        let sender = engine
            .script_container()
            .and_then(|container| container.as_any().downcast_ref::<Transaction>())
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "ContractManagement::deploy requires a transaction container",
                )
            })?
            .sender()
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "ContractManagement::deploy: transaction has no sender",
                )
            })?;
        let nef_bytes = args.first().ok_or_else(|| {
            CoreError::invalid_operation("ContractManagement::deploy requires a NEF file")
        })?;
        let manifest_bytes = args.get(1).ok_or_else(|| {
            CoreError::invalid_operation("ContractManagement::deploy requires a manifest")
        })?;
        if nef_bytes.is_empty() {
            return Err(CoreError::invalid_operation(
                "ContractManagement::deploy: NEF file length cannot be zero",
            ));
        }
        if manifest_bytes.is_empty() {
            return Err(CoreError::invalid_operation(
                "ContractManagement::deploy: manifest length cannot be zero",
            ));
        }
        let data = self.optional_data_arg(engine, args, "deploy")?;

        // C#: AddFee(max(StoragePrice * (nef + manifest), GetMinimumDeploymentFee)
        // * FeeFactor) — the FeeFactor multiplication is the datoshi -> picoGAS
        // conversion that `charge_execution_fee` (datoshi in) performs internally.
        let snapshot = engine.snapshot_cache();
        let payload_len = i64::try_from(nef_bytes.len() + manifest_bytes.len())
            .map_err(|_| CoreError::invalid_operation("deploy payload length overflow"))?;
        let storage_component = i64::from(engine.storage_price())
            .checked_mul(payload_len)
            .ok_or_else(|| CoreError::invalid_operation("deploy storage fee overflow"))?;
        let minimum_fee = self.read_minimum_deployment_fee(&snapshot)?;
        let fee = storage_component.max(minimum_fee);
        engine.charge_execution_fee(u64::try_from(fee).unwrap_or(0))?;

        let nef = Self::parse_nef_checked(nef_bytes, "deploy")?;
        let manifest = Self::parse_manifest_checked(manifest_bytes, "deploy")?;
        // C#: Helper.Check(new Script(nef.Script, HF_Basilisk), manifest.Abi).
        Self::check_script_against_abi(
            &nef.script,
            &manifest.abi,
            engine.is_hardfork_enabled(Hardfork::HfBasilisk),
        )?;
        let hash = Helper::get_contract_hash(&sender, nef.checksum, &manifest.name);

        // C#: Policy.IsBlocked(snapshot, hash) -> "The contract {hash} has been blocked."
        if snapshot
            .get(&crate::PolicyContract::blocked_account_key(&hash))
            .is_some()
        {
            return Err(CoreError::invalid_operation(format!(
                "The contract {hash} has been blocked."
            )));
        }
        let record_key = Self::contract_storage_key(&hash);
        if snapshot.get(&record_key).is_some() {
            return Err(CoreError::invalid_operation(format!(
                "Contract Already Exists: {hash}"
            )));
        }

        let mut contract =
            ContractState::new(self.get_next_available_id(&snapshot)?, hash, nef, manifest);
        contract.update_counter = 0;
        let limits = *engine.execution_limits();
        if !Self::manifest_is_valid(&contract.manifest, &limits, &hash) {
            return Err(CoreError::invalid_operation(format!(
                "Invalid Manifest: {hash}"
            )));
        }

        // The per-contract record plus the big-endian id -> hash index entry.
        snapshot.add(
            record_key,
            StorageItem::from_bytes(Self::serialize_contract_record(&contract)?),
        );
        snapshot.add(Self::contract_id_storage_key(contract.id),
            StorageItem::from_bytes(hash.to_bytes().to_vec()),
        );

        self.on_deploy(engine, &contract, data, false)?;

        Self::contract_state_to_bytes(&contract, "deploy")
    }

    /// C# `ContractManagement.Update(engine, nefFile, manifest, data)` (~312-376):
    /// the CALLING contract updates itself — at least one of `nefFile` /
    /// `manifest` non-null (nullability via the dispatcher's null mask), the
    /// storage fee charged on the payload, the record re-validated
    /// (`Helper.Check` over the final NEF + manifest, name immutable,
    /// `UpdateCounter` capped at u16::MAX and bumped), `Policy.CleanWhitelist`
    /// run, then the `_deploy(data, true)` callback and the `Update` event.
    fn update(&self, engine: &mut ApplicationEngine, args: &[Vec<u8>]) -> CoreResult<Vec<u8>> {
        // Post-Aspidochelone the caller must hold CallFlags.All.
        self.require_call_flags_all(engine, "Update")?;
        let nef_is_null = self.native_arg_is_null(engine, 0);
        let manifest_is_null = self.native_arg_is_null(engine, 1);
        if nef_is_null && manifest_is_null {
            return Err(CoreError::invalid_operation(
                "ContractManagement::update: NEF file and manifest cannot both be null",
            ));
        }
        let nef_bytes = if nef_is_null {
            None
        } else {
            Some(args.first().ok_or_else(|| {
                CoreError::invalid_operation("ContractManagement::update requires a NEF file arg")
            })?)
        };
        let manifest_bytes = if manifest_is_null {
            None
        } else {
            Some(args.get(1).ok_or_else(|| {
                CoreError::invalid_operation("ContractManagement::update requires a manifest arg")
            })?)
        };
        let data = self.optional_data_arg(engine, args, "update")?;

        // C#: AddFee(StoragePrice * FeeFactor * (nef?.len + manifest?.len)) — no
        // minimum-deployment-fee floor for updates.
        let payload_len =
            i64::try_from(nef_bytes.map_or(0, |b| b.len()) + manifest_bytes.map_or(0, |b| b.len()))
                .map_err(|_| CoreError::invalid_operation("update payload length overflow"))?;
        let fee = i64::from(engine.storage_price())
            .checked_mul(payload_len)
            .ok_or_else(|| CoreError::invalid_operation("update storage fee overflow"))?;
        engine.charge_execution_fee(u64::try_from(fee).unwrap_or(0))?;

        // C#: GetAndChange(Prefix_Contract ++ engine.CallingScriptHash) -> the
        // calling contract's record must exist.
        let calling_hash = engine.get_calling_script_hash().ok_or_else(|| {
            CoreError::invalid_operation("ContractManagement::update requires a calling contract")
        })?;
        let snapshot = engine.snapshot_cache();
        let mut contract =
            ContractManagement::get_contract_from_snapshot(&snapshot, &calling_hash)?.ok_or_else(
                || {
                    CoreError::invalid_operation(format!(
                        "Updating Contract Does Not Exist: {calling_hash}"
                    ))
                },
            )?;
        if contract.update_counter == u16::MAX {
            return Err(CoreError::invalid_operation(
                "The contract reached the maximum number of updates.",
            ));
        }

        if let Some(bytes) = nef_bytes {
            contract.nef = Self::parse_nef_checked(bytes, "update")?;
        }
        // C#: Policy.CleanWhitelist(engine, contract) — unconditionally, between
        // the NEF and manifest swaps.
        self.policy_clean_whitelist(engine, &contract)?;
        if let Some(bytes) = manifest_bytes {
            let new_manifest = Self::parse_manifest_checked(bytes, "update")?;
            if new_manifest.name != contract.manifest.name {
                return Err(CoreError::invalid_operation(
                    "The name of the contract can't be changed.",
                ));
            }
            let limits = *engine.execution_limits();
            if !Self::manifest_is_valid(&new_manifest, &limits, &contract.hash) {
                return Err(CoreError::invalid_operation(format!(
                    "Invalid Manifest: {}",
                    contract.hash
                )));
            }
            contract.manifest = new_manifest;
        }
        // C#: Helper.Check over the FINAL nef + manifest combination.
        Self::check_script_against_abi(
            &contract.nef.script,
            &contract.manifest.abi,
            engine.is_hardfork_enabled(Hardfork::HfBasilisk),
        )?;
        contract.update_counter += 1;

        // Persist the updated record (id, hash, and the id index are unchanged)
        // before the queued `_deploy` callback resolves the contract from storage.
        snapshot.update(
            Self::contract_storage_key(&contract.hash),
            StorageItem::from_bytes(Self::serialize_contract_record(&contract)?),
        );

        self.on_deploy(engine, &contract, data, true)?;

        Ok(Vec::new())
    }

    /// C# `contract.InitializeAsync(engine, hardfork)` dispatch for a NON-`ActiveIn`
    /// hardfork scheduled at the persisting block. Audit of every C# native
    /// `InitializeAsync` override (ContractManagement.cs:53, GasToken.cs:29,
    /// NeoToken.cs:106, OracleContract.cs:73, Notary.cs:52, PolicyContract.cs:137):
    /// only `PolicyContract` carries branches for hardforks other than its
    /// `ActiveIn` (the HF_Echidna and HF_Faun re-initializations) — every other
    /// initializer is `if (hardfork == ActiveIn)`-gated, making the non-`ActiveIn`
    /// calls no-ops.
    fn initialize_native_for_hardfork(
        &self,
        engine: &mut ApplicationEngine,
        contract: &dyn NativeContract,
        hardfork: Hardfork,
    ) -> CoreResult<()> {
        if contract.id() == crate::PolicyContract::ID {
            return crate::PolicyContract::new().initialize_for_hardfork(engine, hardfork);
        }
        Ok(())
    }
}

static CONTRACT_MANAGEMENT_METHODS: LazyLock<Vec<NativeMethod>> = LazyLock::new(|| {
    let read_states = CallFlags::READ_STATES.bits();
    vec![
        NativeMethod::new(
            "getContract".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Array,
        )
        .with_parameter_names(["hash"]),
        NativeMethod::new(
            "getContractById".to_string(),
            1 << 15,
            true,
            read_states,
            vec![ContractParameterType::Integer],
            ContractParameterType::Array,
        )
        .with_parameter_names(["id"]),
        NativeMethod::new(
            "getMinimumDeploymentFee".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::Integer,
        ),
        // HF_Echidna added the cheap existence check (CpuFee 1<<14).
        NativeMethod::new(
            "isContract".to_string(),
            1 << 14,
            true,
            read_states,
            vec![ContractParameterType::Hash160],
            ContractParameterType::Boolean,
        )
        .with_active_in(Hardfork::HfEchidna)
        .with_parameter_names(["hash"]),
        // C# HasMethod is ungated; only IsContract is HF_Echidna-gated.
        NativeMethod::new(
            "hasMethod".to_string(),
            1 << 15,
            true,
            read_states,
            vec![
                ContractParameterType::Hash160,
                ContractParameterType::String,
                ContractParameterType::Integer,
            ],
            ContractParameterType::Boolean,
        )
        .with_parameter_names(["hash", "method", "pcount"]),
        // Committee-gated setter: not safe, States, Integer -> Void.
        NativeMethod::new(
            "setMinimumDeploymentFee".to_string(),
            1 << 15,
            false,
            CallFlags::STATES.bits(),
            vec![ContractParameterType::Integer],
            ContractParameterType::Void,
        )
        .with_parameter_names(["value"]),
        // getContractHashes() -> Iterator over (id, hash) for deployed contracts.
        NativeMethod::new(
            "getContractHashes".to_string(),
            1 << 15,
            true,
            read_states,
            vec![],
            ContractParameterType::InteropInterface,
        ),
        // destroy(): the calling contract destroys itself. Not safe,
        // States|AllowNotify, Void.
        NativeMethod::new(
            "destroy".to_string(),
            1 << 15,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![],
            ContractParameterType::Void,
        ),
        // deploy(nefFile, manifest) / deploy(nefFile, manifest, data): C#
        // [ContractMethod(RequiredCallFlags = CallFlags.States |
        // CallFlags.AllowNotify)] — CpuFee 0 (the deployment fee is charged
        // inside the method body), returns the new ContractState (Array).
        NativeMethod::new(
            "deploy".to_string(),
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::ByteArray,
            ],
            ContractParameterType::Array,
        )
        .with_parameter_names(["nefFile", "manifest"]),
        NativeMethod::new(
            "deploy".to_string(),
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::ByteArray,
                ContractParameterType::Any,
            ],
            ContractParameterType::Array,
        )
        .with_parameter_names(["nefFile", "manifest", "data"]),
        // update(nefFile?, manifest?) / update(nefFile?, manifest?, data):
        // same C# attribute shape, Void return; the nullable byte-array args
        // arrive through the dispatcher's null mask.
        NativeMethod::new(
            "update".to_string(),
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::ByteArray,
            ],
            ContractParameterType::Void,
        )
        .with_parameter_names(["nefFile", "manifest"]),
        NativeMethod::new(
            "update".to_string(),
            0,
            false,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits(),
            vec![
                ContractParameterType::ByteArray,
                ContractParameterType::ByteArray,
                ContractParameterType::Any,
            ],
            ContractParameterType::Void,
        )
        .with_parameter_names(["nefFile", "manifest", "data"]),
    ]
});

/// ContractManagement's `[ContractEvent]` declarations
/// (ContractManagement.cs:40-42), all ungated and all carrying a single
/// `Hash` parameter (capital H — the C# attribute argument).
static CONTRACT_MANAGEMENT_EVENTS: LazyLock<Vec<NativeEvent>> = LazyLock::new(|| {
    vec![
        NativeEvent::new(0, "Deploy", &[("Hash", ContractParameterType::Hash160)]),
        NativeEvent::new(1, "Update", &[("Hash", ContractParameterType::Hash160)]),
        NativeEvent::new(2, "Destroy", &[("Hash", ContractParameterType::Hash160)]),
    ]
});

/// The canonical native-contract registration list (C#
/// `NativeContract.Contracts` order: ContractManagement, StdLib, CryptoLib,
/// Ledger, NEO, GAS, Policy, RoleManagement, Oracle, Notary, Treasury), the
/// iteration order of `ContractManagement::on_persist`. Built from the same
/// constructor list the provider registers, so the deployment records and
/// `Deploy`/`Update` notifications follow C#'s contract order.
static NATIVE_CONTRACTS: LazyLock<Vec<std::sync::Arc<dyn NativeContract>>> = LazyLock::new(|| {
    use neo_execution::native_contract_provider::NativeContractProvider;
    crate::provider::StandardNativeProvider::new().all_native_contracts()
});

impl NativeContract for ContractManagement {
    fn id(&self) -> i32 {
        Self::ID
    }

    fn hash(&self) -> UInt160 {
        Self::script_hash()
    }

    fn name(&self) -> &str {
        Self::NAME
    }

    fn methods(&self) -> &[NativeMethod] {
        &CONTRACT_MANAGEMENT_METHODS
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &CONTRACT_MANAGEMENT_EVENTS
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    /// C# `ContractManagement.InitializeAsync(engine, hardfork)` for `hardfork
    /// == ActiveIn` (ContractManagement.cs:53-61; the contract is
    /// genesis-active, so this runs while persisting block 0): seed
    /// `Prefix_MinimumDeploymentFee` (10 GAS) and `Prefix_NextAvailableId` (1).
    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let snapshot = engine.snapshot_cache();
        snapshot.add(
            StorageKey::create(Self::ID, PREFIX_MINIMUM_DEPLOYMENT_FEE),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            ))),
        );
        snapshot.add(
            StorageKey::create(Self::ID, PREFIX_NEXT_AVAILABLE_ID),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_NEXT_AVAILABLE_ID,
            ))),
        );
        Ok(())
    }

    /// C# `ContractManagement.OnPersistAsync` (ContractManagement.cs:71-118):
    /// for every native contract whose `IsInitializeBlock` hits the persisting
    /// block, write (or refresh) its deployment record and emit
    /// `Deploy`/`Update`:
    ///
    /// - no record yet → add the `Prefix_Contract` record (the C#
    ///   interoperable `ContractState` encoding) and the big-endian
    ///   `Prefix_ContractHash` id→hash index entry, then notify `Deploy`;
    /// - record exists (a hardfork refresh) → bump `UpdateCounter` and swap in
    ///   the NEF + manifest composed for this block height (id and hash
    ///   unchanged), then notify `Update`;
    /// - between the record write and the notification, run
    ///   `InitializeAsync(engine, null)` for newly-created genesis-active
    ///   natives and `InitializeAsync(engine, hf)` for every hardfork scheduled
    ///   at this block. Parameterless [`NativeContract::initialize`] models
    ///   the C# `hardfork == ActiveIn` branch; [`initialize_native_for_hardfork`]
    ///   models the non-`ActiveIn` refresh branches such as Policy's
    ///   Echidna/Faun updates.
    fn on_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        let settings = engine.protocol_settings().clone();
        let block_index = engine
            .persisting_block()
            .map(|block| block.index())
            .ok_or_else(|| {
                CoreError::invalid_operation(
                    "ContractManagement::on_persist requires a persisting block",
                )
            })?;

        for contract in NATIVE_CONTRACTS.iter() {
            let (hit, hardforks) = contract.is_initialize_block(&settings, block_index);
            if !hit {
                continue;
            }
            // C# `contract.GetContractState(settings, index)`: the NEF +
            // manifest composed for this block height.
            let composed = neo_execution::native_contract::build_native_contract_state(
                contract.as_ref(),
                &settings,
                block_index,
            );
            let record_key = Self::contract_storage_key(&contract.hash());
            let snapshot = engine.snapshot_cache();
            let existing = snapshot.get(&record_key);
            let is_create = existing.is_none();
            match existing {
                None => {
                    // Create the contract record + the id → hash index entry.
                    snapshot.add(
                        record_key,
                        StorageItem::from_bytes(Self::serialize_contract_record(&composed)?),
                    );
                    snapshot.add(
                        Self::contract_id_storage_key(contract.id()),
                        StorageItem::from_bytes(contract.hash().to_bytes().to_vec()),
                    );

                    // C# create branch: if the native is genesis-active,
                    // `InitializeAsync(engine, null)` runs before the Deploy
                    // notification for this contract.
                    if contract.active_in().is_none() {
                        contract.initialize(engine).map_err(|e| {
                            CoreError::invalid_operation(format!(
                                "ContractManagement::on_persist: initialize {} at block {block_index}: {e}",
                                contract.name()
                            ))
                        })?;
                    }
                }
                Some(item) => {
                    // C#: UpdateCounter++ and the NEF/manifest swap on the
                    // stored record (id, hash, and the id index unchanged).
                    let mut stored = ContractState::deserialize_contract_record(
                        &item.value_bytes(),
                    )
                    .map_err(|e| {
                        CoreError::deserialization(format!(
                            "ContractManagement::on_persist: stored record for {}: {e}",
                            contract.name()
                        ))
                    })?;
                    // C# `oldContract.UpdateCounter++` is unchecked ushort math.
                    stored.update_counter = stored.update_counter.wrapping_add(1);
                    stored.nef = composed.nef;
                    stored.manifest = composed.manifest;
                    snapshot.update(
                        record_key,
                        StorageItem::from_bytes(Self::serialize_contract_record(&stored)?),
                    );
                }
            }

            // C# `foreach (var hf in hfs) await contract.InitializeAsync(engine, hf)`.
            // The `hf == ActiveIn` branch is represented by `initialize()`;
            // other hardfork refresh branches are dispatched explicitly.
            for hardfork in &hardforks {
                if Some(*hardfork) == contract.active_in() {
                    contract.initialize(engine).map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement::on_persist: initialize {} for {hardfork:?} at block {block_index}: {e}",
                            contract.name()
                        ))
                    })?;
                } else {
                    self.initialize_native_for_hardfork(engine, contract.as_ref(), *hardfork)?;
                }
            }

            engine
                .send_notification(
                    Self::script_hash(),
                    if is_create { "Deploy" } else { "Update" }.to_string(),
                    vec![StackItem::from_byte_string(contract.hash().to_bytes())],
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "ContractManagement::on_persist: notify for {}: {e}",
                        contract.name()
                    ))
                })?;
        }
        Ok(())
    }

    /// Resolves a deployed contract's state from storage.
    ///
    /// ContractManagement owns the per-contract records, so it backs the
    /// engine's `fetch_contract` storage path (via the native-contract
    /// provider seam): `System.Contract.Call` to any deployed contract —
    /// native or user — resolves its NEF/manifest through here. Delegates to
    /// the read helper used by the `getContract` invoke arm.
    fn lookup_contract_state(
        &self,
        snapshot: &DataCache,
        hash: &UInt160,
    ) -> CoreResult<Option<ContractState>> {
        Self::get_contract_from_snapshot(snapshot, hash)
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        let snapshot = engine.snapshot_cache();
        match method {
            "getContract" => {
                let hash = Self::parse_hash_arg(args, "getContract")?;
                // C# `GetContract` returns the ContractState (as an Array via
                // ToStackItem) or null on miss; the native return marshaling
                // encodes a null Array result as an empty payload.
                match Self::get_contract_from_snapshot(&snapshot, &hash)? {
                    Some(state) => Self::contract_state_to_bytes(&state, "getContract"),
                    None => Ok(Vec::new()),
                }
            }
            "getContractById" => {
                // C# `GetContractById` maps the id to a hash via the
                // contract-id index, then returns that ContractState (or null).
                let id = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "ContractManagement::getContractById requires an integer id",
                        )
                    })?;
                match Self::get_contract_by_id_from_snapshot(&snapshot, id)? {
                    Some(state) => Self::contract_state_to_bytes(&state, "getContractById"),
                    None => Ok(Vec::new()),
                }
            }
            "getMinimumDeploymentFee" => {
                let fee = self.read_minimum_deployment_fee(&snapshot)?;
                Ok(BigInt::from(fee).to_signed_bytes_le())
            }
            "setMinimumDeploymentFee" => {
                // C#: validate value >= 0 -> AssertCommittee -> overwrite
                // Prefix_MinimumDeploymentFee (stored as the full BigInteger).
                let value = args
                    .first()
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "ContractManagement::setMinimumDeploymentFee requires a value",
                        )
                    })?;
                if value < BigInt::from(0) {
                    return Err(CoreError::invalid_operation(
                        "MinimumDeploymentFee cannot be negative",
                    ));
                }
                crate::committee::assert_committee(engine, "setMinimumDeploymentFee")?;
                self.put_minimum_deployment_fee(&engine.snapshot_cache(), &value)?;
                Ok(Vec::new())
            }
            "getContractHashes" => {
                // C# GetContractHashes: an iterator over Prefix_ContractHash with
                // FindOptions.RemovePrefix and prefix length 1, yielding
                // Struct[id_bytes, hash]. The 4-byte iterator id is decoded back
                // into an InteropInterface (StorageIterator) by the dispatcher.
                let results = self.contract_hash_entries(&snapshot);
                let iterator_id = engine
                    .create_storage_iterator_with_options(results, 1, FindOptions::RemovePrefix)
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement::getContractHashes: {e}"
                        ))
                    })?;
                Ok(iterator_id.to_le_bytes().to_vec())
            }
            "isContract" => {
                let hash = Self::parse_hash_arg(args, "isContract")?;
                // C# `IsContract` = snapshot.Contains(key(Prefix_Contract, hash)).
                Ok(vec![u8::from(Self::is_contract(&snapshot, &hash))])
            }
            "hasMethod" => {
                let hash = Self::parse_hash_arg(args, "hasMethod")?;
                let method_name = String::from_utf8(
                    args.get(1)
                        .ok_or_else(|| {
                            CoreError::invalid_operation(
                                "ContractManagement::hasMethod requires a method name",
                            )
                        })?
                        .clone(),
                )
                .map_err(|e| {
                    CoreError::invalid_operation(format!(
                        "ContractManagement::hasMethod: bad method name: {e}"
                    ))
                })?;
                let pcount = args
                    .get(2)
                    .map(|b| BigInt::from_signed_bytes_le(b))
                    .and_then(|b| b.to_i32())
                    .ok_or_else(|| {
                        CoreError::invalid_operation(
                            "ContractManagement::hasMethod requires a parameter count",
                        )
                    })?;
                // C#: false if the contract does not exist; otherwise whether its
                // manifest ABI declares the (method, pcount) method.
                let has = match Self::get_contract_from_snapshot(&snapshot, &hash)? {
                    Some(state) => Self::abi_has_method(&state.manifest, &method_name, pcount),
                    None => false,
                };
                Ok(vec![u8::from(has)])
            }
            // Both deploy overloads land here; args.len() (2 vs 3) selects
            // the C# overload — the 2-arg form forwards data = StackItem.Null.
            "deploy" => self.deploy(engine, args),
            // Both update overloads land here, same overload convention.
            "update" => self.update(engine, args),
            "destroy" => {
                // C# Destroy (~382): the CALLING contract destroys itself
                // (hash = engine.CallingScriptHash; a missing calling context
                // is the C# null-deref fault).
                let hash = engine.get_calling_script_hash().ok_or_else(|| {
                    CoreError::invalid_operation(
                        "ContractManagement::destroy requires a calling contract",
                    )
                })?;
                // C#: `if (contract is null) return;` — a non-contract caller
                // is a successful no-op.
                let Some(contract) = Self::get_contract_from_snapshot(&snapshot, &hash)? else {
                    return Ok(Vec::new());
                };
                // Delete the per-contract record and the id -> hash index entry.
                snapshot.delete(&Self::contract_storage_key(&hash));
                snapshot.delete(&Self::contract_id_storage_key(contract.id));
                // Delete ALL of the contract's own storage (C# Find over
                // `StorageKey.CreateSearchPrefix(contract.Id, empty)`).
                let search_prefix = StorageKey::new(contract.id, Vec::new());
                let keys: Vec<StorageKey> = snapshot
                    .find(Some(&search_prefix), SeekDirection::Forward)
                    .map(|(key, _)| key)
                    .collect();
                for key in keys {
                    snapshot.delete(&key);
                }
                // C#: `await Policy.BlockAccountInternal(engine, hash)` — lock
                // the destroyed contract (the bool result is discarded) — then
                // `Policy.CleanWhitelist(engine, contract)`.
                crate::PolicyContract::new().block_account_internal(engine, &hash)?;
                self.policy_clean_whitelist(engine, &contract)?;
                // Emit the Destroy event with the destroyed hash.
                engine
                    .send_notification(
                        Self::script_hash(),
                        "Destroy".to_string(),
                        vec![StackItem::from_byte_string(hash.to_bytes())],
                    )
                    .map_err(|e| {
                        CoreError::invalid_operation(format!(
                            "ContractManagement::destroy: notify: {e}"
                        ))
                    })?;
                Ok(Vec::new())
            }
            other => Err(CoreError::invalid_operation(format!(
                "ContractManagement method '{other}' is not implemented"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::StorageItem;
    use neo_vm::StackItem;

    #[test]
    fn native_contract_surface() {
        let c = ContractManagement::new();
        let names: Vec<&str> = c.methods().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "getContract",
                "getContractById",
                "getMinimumDeploymentFee",
                "isContract",
                "hasMethod",
                "setMinimumDeploymentFee",
                "getContractHashes",
                "destroy",
                "deploy",
                "deploy",
                "update",
                "update"
            ]
        );
        // getContractHashes is a safe, ReadStates, no-arg iterator reader.
        let hashes = c
            .methods()
            .iter()
            .find(|m| m.name == "getContractHashes")
            .unwrap();
        assert!(hashes.safe && hashes.active_in.is_none());
        assert!(hashes.parameters.is_empty());
        assert_eq!(hashes.return_type, ContractParameterType::InteropInterface);
        assert_eq!(hashes.required_call_flags, CallFlags::READ_STATES.bits());
        // The committee-gated setter: not safe, States, Integer -> Void.
        let setter = c
            .methods()
            .iter()
            .find(|m| m.name == "setMinimumDeploymentFee")
            .unwrap();
        assert!(!setter.safe);
        assert_eq!(setter.required_call_flags, CallFlags::STATES.bits());
        assert_eq!(setter.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(setter.return_type, ContractParameterType::Void);
        assert_eq!(setter.cpu_fee, 1 << 15);
        assert!(setter.active_in.is_none());
        let has_method = c.methods().iter().find(|m| m.name == "hasMethod").unwrap();
        assert!(has_method.active_in.is_none());
        assert_eq!(has_method.return_type, ContractParameterType::Boolean);
        assert_eq!(has_method.parameters.len(), 3);

        let get_contract = c
            .methods()
            .iter()
            .find(|m| m.name == "getContract")
            .unwrap();
        assert_eq!(
            get_contract.parameters,
            vec![ContractParameterType::Hash160]
        );
        assert_eq!(get_contract.return_type, ContractParameterType::Array);
        assert_eq!(get_contract.cpu_fee, 1 << 15);
        assert!(get_contract.safe && get_contract.active_in.is_none());

        let by_id = c
            .methods()
            .iter()
            .find(|m| m.name == "getContractById")
            .unwrap();
        assert_eq!(by_id.parameters, vec![ContractParameterType::Integer]);
        assert_eq!(by_id.return_type, ContractParameterType::Array);
        assert_eq!(by_id.cpu_fee, 1 << 15);

        let is_contract = c.methods().iter().find(|m| m.name == "isContract").unwrap();
        assert_eq!(is_contract.parameters, vec![ContractParameterType::Hash160]);
        assert_eq!(is_contract.return_type, ContractParameterType::Boolean);
        assert_eq!(is_contract.cpu_fee, 1 << 14);
        assert_eq!(is_contract.active_in, Some(Hardfork::HfEchidna));

        let mut hardforks = std::collections::HashMap::new();
        hardforks.insert(Hardfork::HfEchidna, 100);
        let settings = neo_config::ProtocolSettings {
            hardforks,
            ..neo_config::ProtocolSettings::csharp_default()
        };
        let pre_echidna_state =
            neo_execution::native_contract::build_native_contract_state(&c, &settings, 0);
        assert!(ContractManagement::abi_has_method(
            &pre_echidna_state.manifest,
            "hasMethod",
            3
        ));
        assert!(!ContractManagement::abi_has_method(
            &pre_echidna_state.manifest,
            "isContract",
            1
        ));

        // destroy(): not safe, States|AllowNotify, no params, Void, no hardfork
        // (C# [ContractMethod(CpuFee = 1 << 15,
        // RequiredCallFlags = CallFlags.States | CallFlags.AllowNotify)]).
        let destroys: Vec<_> = c.methods().iter().filter(|m| m.name == "destroy").collect();
        assert_eq!(destroys.len(), 1);
        let destroy = destroys[0];
        assert!(!destroy.safe);
        assert_eq!(
            destroy.required_call_flags,
            (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
        );
        assert!(destroy.parameters.is_empty());
        assert_eq!(destroy.return_type, ContractParameterType::Void);
        assert_eq!(destroy.cpu_fee, 1 << 15);
        assert!(destroy.active_in.is_none());
        assert!(destroy.deprecated_in.is_none());

        // deploy x2 / update x2: C# [ContractMethod(RequiredCallFlags =
        // CallFlags.States | CallFlags.AllowNotify)] — CpuFee/StorageFee 0
        // (fees are charged inside the body), not safe, no hardfork gate.
        let deploys: Vec<_> = c.methods().iter().filter(|m| m.name == "deploy").collect();
        assert_eq!(deploys.len(), 2);
        let updates: Vec<_> = c.methods().iter().filter(|m| m.name == "update").collect();
        assert_eq!(updates.len(), 2);
        for method in deploys.iter().chain(updates.iter()) {
            assert!(!method.safe);
            assert_eq!(method.cpu_fee, 0);
            assert_eq!(method.storage_fee, 0);
            assert_eq!(
                method.required_call_flags,
                (CallFlags::STATES | CallFlags::ALLOW_NOTIFY).bits()
            );
            assert!(method.active_in.is_none());
            assert_eq!(method.parameters[0], ContractParameterType::ByteArray);
            assert_eq!(method.parameters[1], ContractParameterType::ByteArray);
        }
        // Overloads: (nef, manifest) and (nef, manifest, data: Any).
        assert_eq!(deploys[0].parameters.len(), 2);
        assert_eq!(deploys[1].parameters.len(), 3);
        assert_eq!(deploys[1].parameters[2], ContractParameterType::Any);
        assert_eq!(updates[0].parameters.len(), 2);
        assert_eq!(updates[1].parameters.len(), 3);
        assert_eq!(updates[1].parameters[2], ContractParameterType::Any);
        // deploy returns the new ContractState (Array); update is Void.
        assert!(
            deploys
                .iter()
                .all(|m| m.return_type == ContractParameterType::Array)
        );
        assert!(
            updates
                .iter()
                .all(|m| m.return_type == ContractParameterType::Void)
        );
    }

    #[test]
    fn clean_whitelist_storage_decode_uses_stack_value_projection() {
        let source = include_str!("contract_management.rs");
        let start = source
            .find("fn policy_clean_whitelist")
            .expect("policy_clean_whitelist exists");
        let end = source[start..]
            .find("fn read_required_i64_setting")
            .map(|offset| start + offset)
            .expect("following helper exists");
        let helper = &source[start..end];

        assert!(helper.contains("deserialize_stack_value_with_limits"));
        assert!(helper.contains("StackValue::Struct"));
        assert!(!helper.contains("BinarySerializer::deserialize("));
        assert!(!helper.contains("StackItem::Struct"));
    }

    #[test]
    fn get_contract_miss_returns_none() {
        // C# `GetContract` returns null for an unknown hash; the invoke arm maps
        // `None` to an empty payload, which the engine decodes to StackItem::Null.
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[7u8; 20]).unwrap();
        assert!(
            ContractManagement::get_contract_from_snapshot(&cache, &hash)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn get_contract_by_id_miss_returns_none() {
        // C# `GetContractById` returns null when the id has no hash-index entry;
        // the invoke arm maps that to an empty payload (StackItem::Null).
        let cache = DataCache::new(false);
        assert!(
            ContractManagement::get_contract_by_id_from_snapshot(&cache, 42)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn contract_hash_entries_scopes_to_prefix_contract_hash() {
        let cache = DataCache::new(false);
        // Two Prefix_ContractHash entries (id -> hash) plus an unrelated
        // Prefix_Contract entry that must NOT appear in the iterator's backing set.
        let k1 = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, 1);
        let k2 = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, 2);
        cache.add(k1, StorageItem::from_bytes(vec![0xAA; 20]));
        cache.add(
            k2,
            StorageItem::from_bytes(vec![0xBB; 20]),
        );
        cache.add(
            ContractManagement::contract_storage_key(&UInt160::zero()),
            StorageItem::from_bytes(vec![1]),
        );

        let entries = ContractManagement::new().contract_hash_entries(&cache);
        assert_eq!(
            entries.len(),
            2,
            "only Prefix_ContractHash entries are included"
        );
        // Forward-seek order: id 1 before id 2 (big-endian id keys sort ascending).
        assert_eq!(entries[0].1.value_bytes().to_vec(), vec![0xAA; 20]);
        assert_eq!(entries[1].1.value_bytes().to_vec(), vec![0xBB; 20]);
    }

    #[test]
    fn contract_hash_entries_skips_native_negative_ids() {
        // C# GetContractHashes filters `ReadInt32BigEndian(key.Key[1..]) >= 0`:
        // native contracts (negative ids) never appear in the iterator.
        let cache = DataCache::new(false);
        for id in [-1i32, -11] {
            let key = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, id);
            cache.add(key, StorageItem::from_bytes(vec![0xCC; 20]));
        }
        let user = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, 1);
        cache.add(user, StorageItem::from_bytes(vec![0xDD; 20]));

        let entries = ContractManagement::new().contract_hash_entries(&cache);
        assert_eq!(entries.len(), 1, "native (negative-id) entries are skipped");
        assert_eq!(entries[0].1.value_bytes().to_vec(), vec![0xDD; 20]);
        // id 0 is the boundary: C# keeps `Id >= 0`.
        let zero = StorageKey::create_with_int32(ContractManagement::ID, PREFIX_CONTRACT_HASH, 0);
        cache.add(zero, StorageItem::from_bytes(vec![0xEE; 20]));
        assert_eq!(
            ContractManagement::new()
                .contract_hash_entries(&cache)
                .len(),
            2
        );
    }

    #[test]
    fn get_contract_by_id_round_trips_through_the_id_index() {
        // Deploy-shaped fixture: the per-contract record (prefix 8) plus the
        // big-endian id -> hash index entry (prefix 12), as written by C#
        // Deploy; GetContractById resolves the id through the index and then
        // dereferences the hash.
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[0x42u8; 20]).unwrap();
        let state = ContractState::new_native(7, hash, "TestUserContract".to_string());
        cache.add(
            ContractManagement::contract_storage_key(&hash),
            StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
        );
        cache.add(
            ContractManagement::contract_id_storage_key(7),
            StorageItem::from_bytes(hash.to_bytes().to_vec()),
        );

        let fetched = ContractManagement::get_contract_by_id_from_snapshot(&cache, 7)
            .unwrap()
            .expect("id 7 resolves to the deployed contract");
        assert_eq!(fetched.id, 7);
        assert_eq!(fetched.hash, hash);
        // A different id still misses.
        assert!(
            ContractManagement::get_contract_by_id_from_snapshot(&cache, 8)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn get_contract_by_id_ignores_legacy_little_endian_index_like_csharp_v3100() {
        // C# v3.10 uses StorageKey.Create(id, prefix, int), which appends the
        // contract id in big-endian form. A little-endian compatibility key is
        // not a valid v3.10 lookup path.
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[0x24u8; 20]).unwrap();
        let state = ContractState::new_native(7, hash, "LegacyIndexFixture".to_string());
        cache.add(
            ContractManagement::contract_storage_key(&hash),
            StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
        );
        // Legacy entry written with a LITTLE-endian id suffix (historical bug);
        // modern entries use big-endian. `contract_hash_entries` must still skip it.
        let legacy_key = StorageKey::create_with_bytes(
            ContractManagement::ID,
            PREFIX_CONTRACT_HASH,
            &7i32.to_le_bytes(),
        );
        cache.add(
            legacy_key,
            StorageItem::from_bytes(hash.to_bytes().to_vec()),
        );

        assert!(
            ContractManagement::get_contract_by_id_from_snapshot(&cache, 7)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn has_method_resolves_contract_from_snapshot() {
        use neo_manifest::{ContractMethodDescriptor, ContractParameterDefinition};
        // The hasMethod invoke arm = GetContract(hash) -> Abi.GetMethod(name,
        // pcount) != null; exercise the same composition over a seeded record.
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[0x51u8; 20]).unwrap();
        let mut state = ContractState::new_native(9, hash, "HasMethodFixture".to_string());
        state.manifest.abi.methods.push(ContractMethodDescriptor {
            name: "transfer".to_string(),
            parameters: vec![ContractParameterDefinition::default(); 4],
            ..Default::default()
        });
        cache.add(
            ContractManagement::contract_storage_key(&hash),
            StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
        );

        let fetched = ContractManagement::get_contract_from_snapshot(&cache, &hash)
            .unwrap()
            .expect("contract record resolves");
        // Positive: exact pcount and the -1 wildcard.
        assert!(ContractManagement::abi_has_method(
            &fetched.manifest,
            "transfer",
            4
        ));
        assert!(ContractManagement::abi_has_method(
            &fetched.manifest,
            "transfer",
            -1
        ));
        // Negative: wrong pcount / unknown name.
        assert!(!ContractManagement::abi_has_method(
            &fetched.manifest,
            "transfer",
            3
        ));
        assert!(!ContractManagement::abi_has_method(
            &fetched.manifest,
            "balanceOf",
            -1
        ));
        // Missing contract -> C# returns false before any ABI lookup.
        let absent = UInt160::from_bytes(&[0x52u8; 20]).unwrap();
        assert!(
            ContractManagement::get_contract_from_snapshot(&cache, &absent)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn has_method_rejects_invalid_utf8_method_name_like_csharp() {
        // C# NativeContract.Invoke converts string parameters through
        // StackItem.GetString(), so invalid UTF-8 faults instead of being repaired.
        let cache = DataCache::new(false);
        let mut engine = ApplicationEngine::new(
            neo_primitives::TriggerType::Application,
            None,
            std::sync::Arc::new(cache),
            None,
            neo_config::ProtocolSettings::default(),
            0,
            None,
        )
        .expect("engine builds");
        let hash = UInt160::from_bytes(&[0x51u8; 20]).unwrap();
        let err = ContractManagement::new()
            .invoke(
                &mut engine,
                "hasMethod",
                &[
                    hash.to_bytes().to_vec(),
                    vec![0xFF],
                    BigInt::from(0).to_signed_bytes_le(),
                ],
            )
            .expect_err("invalid UTF-8 method names must fault");
        assert!(err.to_string().contains("bad method name"), "{err}");
    }

    #[test]
    fn is_native_contract_hash_covers_all_eleven_natives() {
        for spec in crate::standard_native_contract_specs() {
            assert!(
                ContractManagement::is_native_contract_hash(&spec.hash),
                "{} is native",
                spec.name
            );
        }
        let user = UInt160::from_bytes(&[0x99u8; 20]).unwrap();
        assert!(!ContractManagement::is_native_contract_hash(&user));
    }

    #[test]
    fn policy_blocked_account_key_matches_policy_layout() {
        // The cross-native blocked-account key must match PolicyContract's own
        // layout: (PolicyContract.ID, [Prefix_BlockedAccount(15), account]).
        let account = UInt160::from_bytes(&[0x77u8; 20]).unwrap();
        let key = crate::PolicyContract::blocked_account_key(&account);
        assert_eq!(key.id, crate::PolicyContract::ID);
        assert_eq!(key.suffix()[0], POLICY_PREFIX_BLOCKED_ACCOUNT);
        assert_eq!(&key.suffix()[1..], account.to_bytes().as_slice());
    }

    #[test]
    fn set_minimum_deployment_fee_write_round_trips() {
        // The setter's storage effect (overwrite Prefix_MinimumDeploymentFee) is
        // observed by the getMinimumDeploymentFee reader, matching C#
        // GetAndChange(...).Set(value).
        let cache = DataCache::new(false);
        cache.add(
            StorageKey::create(ContractManagement::ID, PREFIX_MINIMUM_DEPLOYMENT_FEE),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            ))),
        );
        // Zero is permitted (C# rejects only value < 0).
        ContractManagement::new()
            .put_minimum_deployment_fee(&cache, &BigInt::from(0))
            .unwrap();
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_MINIMUM_DEPLOYMENT_FEE,
                DEFAULT_MINIMUM_DEPLOYMENT_FEE
            )
            .unwrap(),
            0
        );
        // Overwrite with a positive fee (GetAndChange semantics).
        ContractManagement::new()
            .put_minimum_deployment_fee(&cache, &BigInt::from(25_00000000i64))
            .unwrap();
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_MINIMUM_DEPLOYMENT_FEE,
                DEFAULT_MINIMUM_DEPLOYMENT_FEE
            )
            .unwrap(),
            25_00000000
        );
    }

    #[test]
    fn abi_has_method_matches_name_and_pcount() {
        use neo_manifest::{
            ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
        };
        let mut manifest = ContractManifest::new("test".to_string());
        manifest.abi.methods.push(ContractMethodDescriptor {
            name: "transfer".to_string(),
            parameters: vec![ContractParameterDefinition::default(); 4],
            ..Default::default()
        });

        // Exact (name, count) match.
        assert!(ContractManagement::abi_has_method(&manifest, "transfer", 4));
        // Wrong count -> no match.
        assert!(!ContractManagement::abi_has_method(
            &manifest, "transfer", 3
        ));
        // pcount == -1 matches any count.
        assert!(ContractManagement::abi_has_method(
            &manifest, "transfer", -1
        ));
        // Unknown name -> no match.
        assert!(!ContractManagement::abi_has_method(
            &manifest,
            "balanceOf",
            -1
        ));
        // Empty manifest -> no match.
        assert!(!ContractManagement::abi_has_method(
            &ContractManifest::new("e".to_string()),
            "transfer",
            -1
        ));
    }

    #[test]
    fn is_contract_checks_storage_existence() {
        let cache = DataCache::new(false);
        let hash = UInt160::from_bytes(&[8u8; 20]).unwrap();
        assert!(!ContractManagement::is_contract(&cache, &hash));
        cache.add(
            ContractManagement::contract_storage_key(&hash),
            StorageItem::from_bytes(vec![1]),
        );
        assert!(ContractManagement::is_contract(&cache, &hash));
    }

    #[test]
    fn contract_state_marshals_to_five_element_array() {
        // getContract's hit path serializes the same 5-field Array
        // (id, updateCounter, hash, nef, manifest) as C# ContractState.ToStackItem.
        let state = ContractState::default();
        let legacy_item = StackItem::try_from(state.to_stack_value()).unwrap();
        let expected =
            BinarySerializer::serialize(&legacy_item, &ExecutionEngineLimits::default()).unwrap();
        let bytes = ContractManagement::contract_state_to_bytes(&state, "test").unwrap();
        assert_eq!(bytes, expected);
        assert!(!bytes.is_empty());
        let decoded =
            BinarySerializer::deserialize(&bytes, &ExecutionEngineLimits::default(), None).unwrap();
        match decoded {
            StackItem::Array(array) => assert_eq!(array.items().len(), 5),
            other => panic!("expected Array, got {other:?}"),
        }

        let source = include_str!("contract_management.rs");
        let start = source
            .find("fn contract_state_to_bytes")
            .expect("contract_state_to_bytes helper exists");
        let end = source[start..]
            .find("fn contract_hash_entries")
            .map(|offset| start + offset)
            .expect("contract_hash_entries follows contract_state_to_bytes");
        let helper = &source[start..end];

        assert!(helper.contains("to_stack_value"));
        assert!(helper.contains("serialize_stack_value_default"));
        assert!(!helper.contains("to_stack_item"));
        assert!(!helper.contains("BinarySerializer::serialize("));
    }

    #[test]
    fn minimum_deployment_fee_requires_initialized_storage() {
        let cache = DataCache::new(false);
        let mut engine = ApplicationEngine::new(
            neo_primitives::TriggerType::Application,
            None,
            std::sync::Arc::new(cache),
            None,
            neo_config::ProtocolSettings::default(),
            0,
            None,
        )
        .expect("engine builds");

        let err = ContractManagement::new()
            .invoke(&mut engine, "getMinimumDeploymentFee", &[])
            .expect_err("missing minimum deployment fee storage should fault");
        assert!(err.to_string().contains("MinimumDeploymentFee"), "{err}");
    }

    /// A minimal deployable manifest: one `main()` method at offset 0 (the
    /// ABI must be non-empty, C# `ContractAbi.FromJson`).
    fn deployable_manifest(name: &str) -> ContractManifest {
        use neo_manifest::ContractMethodDescriptor;
        let mut manifest = ContractManifest::new(name.to_string());
        manifest.abi.methods.push(
            ContractMethodDescriptor::new(
                "main".to_string(),
                vec![],
                ContractParameterType::Void,
                0,
                true,
            )
            .expect("method descriptor"),
        );
        manifest
    }

    #[test]
    fn next_available_id_requires_initialized_storage_then_increments() {
        // C# GetNextAvailableId: return the stored value, write value + 1.
        let cache = DataCache::new(false);
        let err = ContractManagement::new()
            .get_next_available_id(&cache)
            .expect_err("missing next available id storage should fault");
        assert!(err.to_string().contains("NextAvailableId"), "{err}");

        cache.add(
            StorageKey::create(ContractManagement::ID, PREFIX_NEXT_AVAILABLE_ID),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_NEXT_AVAILABLE_ID,
            ))),
        );
        assert_eq!(
            ContractManagement::new()
                .get_next_available_id(&cache)
                .unwrap(),
            1
        );
        assert_eq!(
            ContractManagement::new()
                .get_next_available_id(&cache)
                .unwrap(),
            2
        );
        assert_eq!(
            crate::read_storage_int(
                &cache,
                ContractManagement::ID,
                PREFIX_NEXT_AVAILABLE_ID,
                DEFAULT_NEXT_AVAILABLE_ID
            )
            .unwrap(),
            3
        );
    }

    #[test]
    fn check_script_against_abi_validates_offsets_and_uniqueness() {
        use neo_manifest::{ContractEventDescriptor, ContractMethodDescriptor};
        let method = |name: &str, offset: i32| {
            ContractMethodDescriptor::new(
                name.to_string(),
                vec![],
                ContractParameterType::Void,
                offset,
                true,
            )
            .unwrap()
        };
        let ret_script = vec![neo_vm_rs::OpCode::RET.byte()];

        // A method at offset 0 (RET) passes in both strict and lazy modes.
        let abi = ContractAbi::new(vec![method("main", 0)], vec![]);
        assert!(ContractManagement::check_script_against_abi(&ret_script, &abi, true).is_ok());
        assert!(ContractManagement::check_script_against_abi(&ret_script, &abi, false).is_ok());

        // An out-of-range offset fails in both modes (C# `ip >= Length`).
        let abi_oob = ContractAbi::new(vec![method("main", 9)], vec![]);
        assert!(ContractManagement::check_script_against_abi(&ret_script, &abi_oob, true).is_err());
        assert!(
            ContractManagement::check_script_against_abi(&ret_script, &abi_oob, false).is_err()
        );

        // PUSHDATA1 [len 1] [0x40]: offset 2 sits INSIDE the operand. The
        // strict (post-Basilisk) Script rejects non-boundary offsets, while the
        // pre-Basilisk lazy Script parses the byte at 2 as RET and accepts —
        // the exact C# Script strict-mode divergence.
        let pushdata = vec![0x0C, 0x01, 0x40];
        let abi_mid = ContractAbi::new(vec![method("main", 2)], vec![]);
        assert!(ContractManagement::check_script_against_abi(&pushdata, &abi_mid, true).is_err());
        assert!(ContractManagement::check_script_against_abi(&pushdata, &abi_mid, false).is_ok());

        // Duplicate (name, pcount) pairs fail (C# methodDictionary build).
        let abi_dup = ContractAbi::new(vec![method("main", 0), method("main", 0)], vec![]);
        assert!(ContractManagement::check_script_against_abi(&ret_script, &abi_dup, true).is_err());

        // Duplicate event names fail (C# events ToDictionary).
        let abi_dup_events = ContractAbi::new(
            vec![method("main", 0)],
            vec![
                ContractEventDescriptor::new("Changed".to_string(), vec![]).unwrap(),
                ContractEventDescriptor::new("Changed".to_string(), vec![]).unwrap(),
            ],
        );
        assert!(
            ContractManagement::check_script_against_abi(&ret_script, &abi_dup_events, true)
                .is_err()
        );
    }

    #[test]
    fn manifest_is_valid_checks_serialization_and_group_signatures() {
        use neo_crypto::ECPoint;
        use neo_manifest::ContractGroup;
        let limits = ExecutionEngineLimits::default();
        let hash = UInt160::from_bytes(&[0x21u8; 20]).unwrap();

        // No groups: valid (the stack-item projection serializes within limits).
        assert!(ContractManagement::manifest_is_valid(
            &deployable_manifest("Valid"),
            &limits,
            &hash
        ));

        // A group whose signature does not verify against the contract hash
        // makes the manifest invalid (C# Groups.All(u => u.IsValid(hash))).
        let pub_key = ECPoint::from_bytes(
            &hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c")
                .unwrap(),
        )
        .unwrap();
        let mut bad = deployable_manifest("Valid");
        bad.groups.push(ContractGroup::new(pub_key, vec![0xAB; 64]));
        assert!(!ContractManagement::manifest_is_valid(&bad, &limits, &hash));
    }

    #[test]
    fn parse_manifest_checked_enforces_csharp_parse_gates() {
        // Empty payload (C# "Manifest length cannot be zero").
        assert!(ContractManagement::parse_manifest_checked(&[], "deploy").is_err());
        // Over the u16::MAX byte cap of C# ContractManifest.Parse.
        let oversized = vec![b' '; MAX_MANIFEST_LENGTH + 1];
        assert!(ContractManagement::parse_manifest_checked(&oversized, "deploy").is_err());
        // Not UTF-8 / not JSON.
        assert!(ContractManagement::parse_manifest_checked(&[0xFF, 0xFE], "deploy").is_err());
        // Structurally invalid: an empty ABI (C# ContractAbi.FromJson throws).
        let empty_abi = ContractManifest::new("NoMethods".to_string())
            .to_json()
            .unwrap()
            .to_string()
            .into_bytes();
        assert!(ContractManagement::parse_manifest_checked(&empty_abi, "deploy").is_err());
        // A valid manifest parses and keeps its name + ABI.
        let bytes = deployable_manifest("RoundTrip")
            .to_json()
            .unwrap()
            .to_string()
            .into_bytes();
        let parsed = ContractManagement::parse_manifest_checked(&bytes, "deploy").unwrap();
        assert_eq!(parsed.name, "RoundTrip");
        assert_eq!(parsed.abi.methods.len(), 1);
    }

    #[test]
    fn parse_nef_checked_validates_container_and_checksum() {
        // Empty payload (C# "NEF file length cannot be zero").
        assert!(ContractManagement::parse_nef_checked(&[], "deploy").is_err());
        // A valid NEF3 container round-trips.
        let nef = NefFile::new("unit-test".to_string(), vec![neo_vm_rs::OpCode::RET.byte()]);
        let bytes = nef.to_bytes();
        let parsed = ContractManagement::parse_nef_checked(&bytes, "deploy").unwrap();
        assert_eq!(parsed.checksum, nef.checksum);
        assert_eq!(parsed.script, vec![neo_vm_rs::OpCode::RET.byte()]);
        // Corrupting the trailing checksum fails the parse (the C#
        // AsSerializable<NefFile> checksum verifier).
        let mut corrupted = bytes;
        let last = corrupted.len() - 1;
        corrupted[last] ^= 0xFF;
        assert!(ContractManagement::parse_nef_checked(&corrupted, "deploy").is_err());
    }
}

/// Engine-level tests for `destroy` and its `Policy.BlockAccountInternal` /
/// `Policy.CleanWhitelist` ports, using the witness-gated script-execution
/// harness proven in `neo_token::governance_writer_tests`.
#[cfg(test)]
mod destroy_engine_tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_io::BinaryWriter;
    use neo_payloads::signer::Signer;
    use neo_payloads::transaction::Transaction;
    use neo_payloads::witness::Witness;
    use neo_payloads::{Block, BlockHeader};
    use neo_primitives::{TriggerType, Verifiable, WitnessScope};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::VmState;
    use std::sync::Arc;

    /// Writes a serialized contract record under `Prefix_Contract ++ hash`.
    fn put_contract_record(cache: &DataCache, state: &ContractState) {
        cache.add(
            ContractManagement::contract_storage_key(&state.hash),
            StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
        );
    }

    /// Builds the entry script `System.Contract.Call(CM, "destroy", [])`.
    fn destroy_script() -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(0);
        builder.emit_pack();
        builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
        builder.emit_push("destroy".as_bytes());
        builder.emit_push(&ContractManagement::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");
        builder.to_array()
    }

    fn engine_for(
        snapshot: Arc<DataCache>,
        persisting_block: Option<Block>,
        settings: ProtocolSettings,
    ) -> ApplicationEngine {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            persisting_block,
            settings,
            100_00000000,
            None,
        )
        .expect("engine builds")
    }

    #[test]
    fn destroy_removes_record_index_storage_and_blocks_hash() {
        crate::install();
        let cache = DataCache::new(false);
        // Seed the ContractManagement native record so System.Contract.Call
        // resolves the callee.
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );

        // The entry script IS the calling contract: pin its hash, then deploy
        // a user contract under that hash (record + id index + one storage
        // row + one Policy whitelist entry).
        let script = destroy_script();
        let self_hash = UInt160::from_script(&script);
        let user = ContractState::new_native(7, self_hash, "SelfDestructFixture".to_string());
        put_contract_record(&cache, &user);
        let index_key = ContractManagement::contract_id_storage_key(7);
        cache.add(
            index_key.clone(),
            StorageItem::from_bytes(self_hash.to_bytes().to_vec()),
        );
        let user_row = StorageKey::new(7, vec![0x01]);
        cache.add(user_row.clone(), StorageItem::from_bytes(vec![0xEE]));
        // A whitelist entry for the contract (C# WhitelistedContract
        // Struct[ContractHash, Method, ArgCount, FixedFee]) that CleanWhitelist
        // must remove and report.
        // Layout: [PREFIX, self_hash160, 0i32_be].
        let mut wl_suffix = Vec::with_capacity(20 + 4);
        wl_suffix.extend_from_slice(&self_hash.to_bytes());
        wl_suffix.extend_from_slice(&0i32.to_be_bytes());
        let wl_key = StorageKey::create_with_bytes(
            crate::PolicyContract::ID,
            POLICY_PREFIX_WHITELISTED_FEE_CONTRACTS,
            &wl_suffix,
        );
        let wl_value = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_byte_string(self_hash.to_bytes()),
                StackItem::from_byte_string("transfer".as_bytes().to_vec()),
                StackItem::from_int(4),
                StackItem::from_int(0),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        cache.add(wl_key.clone(), StorageItem::from_bytes(wl_value));
        let snapshot = Arc::new(cache);

        // Default MainNet schedules Faun at 8,800,000, so height 0 runs the
        // pre-Faun BlockAccountInternal branch (empty blocked value).
        // The destroy path reads the persisting block's timestamp, so the
        // engine needs a persisting block fixture (height 0, pre-Faun).
        let mut persisting_header = BlockHeader::default();
        persisting_header.set_index(0);
        persisting_header.set_timestamp(1_700_000_000_000);
        let persisting_block = Some(Block::from_parts(persisting_header, vec![]));
        let mut engine =
            engine_for(Arc::clone(&snapshot), persisting_block, ProtocolSettings::default());
        engine
            .load_script(script, CallFlags::ALL, Some(self_hash))
            .expect("script loads");
        assert_eq!(
            engine.execute_allow_fault(),
            VmState::HALT,
            "destroy must HALT"
        );

        // The contract record, id index, and contract storage are gone.
        assert!(
            snapshot
                .get(&ContractManagement::contract_storage_key(&self_hash))
                .is_none(),
            "contract record deleted"
        );
        assert!(
            snapshot.get(&index_key).is_none(),
            "id->hash index entry deleted"
        );
        assert!(
            snapshot.get(&user_row).is_none(),
            "contract storage deleted"
        );
        // The destroyed hash is locked via Policy's blocked-account entry,
        // pre-Faun with an EMPTY value (C# StorageItem([])).
        let blocked = snapshot
            .get(&crate::PolicyContract::blocked_account_key(&self_hash))
            .expect("destroyed contract is blocked");
        assert!(
            blocked.value_bytes().is_empty(),
            "pre-Faun blocked value is empty"
        );
        // The whitelist entry was cleaned.
        assert!(snapshot.get(&wl_key).is_none(), "whitelist entry deleted");

        // Events: Policy's WhitelistFeeChanged for the cleaned entry, then
        // ContractManagement's Destroy with the destroyed hash.
        let notifications = engine.notifications();
        let destroy_event = notifications
            .iter()
            .find(|n| n.event_name == "Destroy")
            .expect("Destroy event emitted");
        assert_eq!(destroy_event.script_hash, ContractManagement::script_hash());
        assert_eq!(
            destroy_event.state[0].as_bytes().unwrap(),
            self_hash.to_bytes().to_vec()
        );
        let wl_event = notifications
            .iter()
            .find(|n| n.event_name == "WhitelistFeeChanged")
            .expect("WhitelistFeeChanged event emitted");
        assert_eq!(wl_event.script_hash, crate::PolicyContract::script_hash());
        assert_eq!(wl_event.state[1].as_bytes().unwrap(), b"transfer".to_vec());
        assert_eq!(wl_event.state[2].as_int().unwrap(), BigInt::from(4));
        assert!(matches!(wl_event.state[3], StackItem::Null));
    }

    #[test]
    fn destroy_is_a_noop_for_a_non_contract_caller() {
        crate::install();
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );
        let script = destroy_script();
        let self_hash = UInt160::from_script(&script);
        let snapshot = Arc::new(cache);

        // No contract record for the calling script: C# `if (contract is null)
        // return;` — a successful no-op that writes nothing.
        let mut engine = engine_for(Arc::clone(&snapshot), None, ProtocolSettings::default());
        engine
            .load_script(script, CallFlags::ALL, Some(self_hash))
            .expect("script loads");
        assert_eq!(
            engine.execute_allow_fault(),
            VmState::HALT,
            "no-op destroy HALTs"
        );
        assert!(
            snapshot
                .get(&crate::PolicyContract::blocked_account_key(&self_hash))
                .is_none(),
            "no blocked-account entry for a no-op destroy"
        );
        assert!(
            engine
                .notifications()
                .iter()
                .all(|n| n.event_name != "Destroy"),
            "no Destroy event for a no-op destroy"
        );
    }

    #[test]
    fn block_account_internal_faun_writes_timestamp_and_is_idempotent() {
        crate::install();
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        let mut header = BlockHeader::default();
        header.set_index(1);
        header.set_timestamp(1_700_000_123_456);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = engine_for(
            Arc::clone(&snapshot),
            Some(Block::from_parts(header, vec![])),
            settings,
        );

        let account = UInt160::from_bytes(&[0x33u8; 20]).unwrap();
        // First block: post-Faun the entry stores GetTime() (the persisting
        // block's timestamp) for Policy's recoverFund.
        assert!(
            crate::PolicyContract::new()
                .block_account_internal(&mut engine, &account)
                .unwrap()
        );
        let item = snapshot
            .get(&crate::PolicyContract::blocked_account_key(&account))
            .expect("blocked entry written");
        assert_eq!(
            BigInt::from_signed_bytes_le(&item.value_bytes()),
            BigInt::from(1_700_000_123_456i64)
        );
        // Already blocked -> false, nothing rewritten (C# returns early).
        assert!(
            !crate::PolicyContract::new()
                .block_account_internal(&mut engine, &account)
                .unwrap()
        );
    }

    #[test]
    fn block_account_internal_rejects_native_hashes() {
        crate::install();
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = engine_for(Arc::clone(&snapshot), None, ProtocolSettings::default());
        // C#: "Cannot block a native contract."
        let neo_hash = *crate::hashes::NEO_TOKEN_HASH;
        let err = crate::PolicyContract::new()
            .block_account_internal(&mut engine, &neo_hash)
            .unwrap_err();
        assert!(err.to_string().contains("native"));
        assert!(
            snapshot
                .get(&crate::PolicyContract::blocked_account_key(&neo_hash))
                .is_none()
        );
    }

    #[test]
    fn block_account_internal_faun_runs_vote_transition_for_neo_holders() {
        // C# BlockAccountInternal post-Faun runs NEO.VoteInternal(account,
        // null): for a NEO-holding account the full vote transition executes
        // (here a no-op un-vote — the account votes for nobody), then the
        // blocked entry is written with the persisting block's timestamp.
        crate::install();
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        let mut header = BlockHeader::default();
        header.set_index(1);
        header.set_timestamp(1_700_000_000_000);
        let cache = DataCache::new(false);
        let account = UInt160::from_bytes(&[0x44u8; 20]).unwrap();
        // Seed a NeoToken account state holding 100 NEO.
        let neo_key =
            StorageKey::create_with_uint160(crate::NeoToken::ID, crate::NEP17_PREFIX_ACCOUNT, &account);
        let neo_state = BinarySerializer::serialize(
            &StackItem::from_struct(vec![
                StackItem::from_int(100),
                StackItem::from_int(0),
                StackItem::Null,
                StackItem::from_int(0),
            ]),
            &ExecutionEngineLimits::default(),
        )
        .unwrap();
        cache.add(neo_key, StorageItem::from_bytes(neo_state));
        let snapshot = Arc::new(cache);
        let mut engine = engine_for(
            Arc::clone(&snapshot),
            Some(Block::from_parts(header, vec![])),
            settings,
        );

        assert!(
            crate::PolicyContract::new()
                .block_account_internal(&mut engine, &account)
                .unwrap()
        );
        let item = snapshot
            .get(&crate::PolicyContract::blocked_account_key(&account))
            .expect("blocked entry written after the vote transition");
        assert_eq!(
            BigInt::from_signed_bytes_le(&item.value_bytes()),
            BigInt::from(1_700_000_000_000i64),
            "entry stores GetTime() for recoverFund"
        );
    }
}

/// Engine-level tests for `deploy` / `update`, using the witness-gated
/// script-execution harness proven in `neo_token::governance_writer_tests`:
/// the entry script does `System.Contract.Call(CM, method, args)` against a
/// snapshot seeded with the ContractManagement native record.
#[cfg(test)]
mod deploy_update_engine_tests {
    use super::*;
    use neo_config::ProtocolSettings;
    use neo_execution::native_contract::build_native_contract_state;
    use neo_manifest::{ContractMethodDescriptor, ContractParameterDefinition};
    use neo_payloads::signer::Signer;
    use neo_payloads::witness::Witness;
    use neo_primitives::{TriggerType, Verifiable, WitnessScope};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::{OpCode, VmState};
    use std::sync::Arc;

    /// The deploying transaction's sender (first signer).
    const SENDER: [u8; 20] = [0x07; 20];

    /// Writes a serialized contract record under `Prefix_Contract ++ hash`.
    fn put_contract_record(cache: &DataCache, state: &ContractState) {
        cache.add(
            ContractManagement::contract_storage_key(&state.hash),
            StorageItem::from_bytes(
                ContractManagement::serialize_contract_record(state).expect("record bytes"),
            ),
        );
    }

    fn seed_contract_management_settings(cache: &DataCache) {
        cache.add(
            StorageKey::create(ContractManagement::ID, PREFIX_MINIMUM_DEPLOYMENT_FEE),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            ))),
        );
        cache.add(
            StorageKey::create(ContractManagement::ID, PREFIX_NEXT_AVAILABLE_ID),
            StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(
                DEFAULT_NEXT_AVAILABLE_ID,
            ))),
        );
    }

    /// Snapshot seeded with the ContractManagement native record so
    /// `System.Contract.Call` resolves the callee.
    fn seeded_snapshot() -> Arc<DataCache> {
        crate::install();
        let cache = DataCache::new(false);
        seed_contract_management_settings(&cache);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );
        Arc::new(cache)
    }

    fn faun_from_genesis_settings() -> ProtocolSettings {
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfEchidna, 0);
        settings.hardforks.insert(Hardfork::HfFaun, 0);
        settings
    }

    /// The smallest NEF that parses: a single RET at offset 0.
    fn minimal_nef() -> NefFile {
        NefFile::new("e2e-test".to_string(), vec![OpCode::RET.byte()])
    }

    /// A minimal deployable manifest: `main()` at offset 0.
    fn deployable_manifest(name: &str) -> ContractManifest {
        let mut manifest = ContractManifest::new(name.to_string());
        manifest.abi.methods.push(
            ContractMethodDescriptor::new(
                "main".to_string(),
                vec![],
                ContractParameterType::Void,
                0,
                true,
            )
            .expect("method descriptor"),
        );
        manifest
    }

    /// JSON payload for a manifest (what a deploying transaction carries).
    fn manifest_json(manifest: &ContractManifest) -> Vec<u8> {
        manifest
            .to_json()
            .expect("manifest json")
            .to_string()
            .into_bytes()
    }

    fn engine_for(
        snapshot: Arc<DataCache>,
        settings: ProtocolSettings,
        sender: UInt160,
    ) -> ApplicationEngine {
        let mut tx = Transaction::new();
        tx.set_signers(vec![Signer::new(sender, WitnessScope::GLOBAL)]);
        tx.set_witnesses(vec![Witness::empty()]);
        let container: Arc<dyn Verifiable> = Arc::new(tx);
        ApplicationEngine::new(
            TriggerType::Application,
            Some(container),
            snapshot,
            None,
            settings,
            1000_00000000, // covers the 10-GAS minimum deployment fee
            None,
        )
        .expect("engine builds")
    }

    /// Runs `System.Contract.Call(CM, "deploy", [nef, manifest(, data)])` and
    /// returns the final VM state plus the engine (for fee / notification /
    /// result-stack assertions).
    fn run_deploy(
        snapshot: &Arc<DataCache>,
        settings: ProtocolSettings,
        sender: UInt160,
        nef_bytes: &[u8],
        manifest_bytes: &[u8],
        data: Option<&[u8]>,
        flags: CallFlags,
    ) -> (VmState, ApplicationEngine) {
        let mut builder = ScriptBuilder::new();
        // Args are pushed deepest-first (argN-1 .. arg0) before PACK.
        let argc = if let Some(data) = data {
            builder.emit_push(data);
            3
        } else {
            2
        };
        builder.emit_push(manifest_bytes);
        builder.emit_push(nef_bytes);
        builder.emit_push_int(argc);
        builder.emit_pack();
        builder.emit_push_int(i64::from(flags.bits()));
        builder.emit_push("deploy".as_bytes());
        builder.emit_push(&ContractManagement::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");

        let mut engine = engine_for(Arc::clone(snapshot), settings, sender);
        engine
            .load_script(builder.to_array(), CallFlags::ALL, None)
            .expect("script loads");
        let state = engine.execute_allow_fault();
        (state, engine)
    }

    /// Builds the self-update entry script
    /// `System.Contract.Call(CM, "update", [nef?, manifest?])`; `None` pushes
    /// the C# `null` argument.
    fn update_script(
        nef_bytes: Option<&[u8]>,
        manifest_bytes: Option<&[u8]>,
        flags: CallFlags,
    ) -> Vec<u8> {
        let mut builder = ScriptBuilder::new();
        // arg1 (manifest) deepest, then arg0 (nef) on top, then PACK 2.
        match manifest_bytes {
            Some(bytes) => {
                builder.emit_push(bytes);
            }
            None => {
                builder.emit_opcode(OpCode::PUSHNULL);
            }
        }
        match nef_bytes {
            Some(bytes) => {
                builder.emit_push(bytes);
            }
            None => {
                builder.emit_opcode(OpCode::PUSHNULL);
            }
        }
        builder.emit_push_int(2);
        builder.emit_pack();
        builder.emit_push_int(i64::from(flags.bits()));
        builder.emit_push("update".as_bytes());
        builder.emit_push(&ContractManagement::script_hash().to_array());
        builder
            .emit_syscall("System.Contract.Call")
            .expect("System.Contract.Call");
        builder.to_array()
    }

    /// Runs a self-update entry script whose pinned hash is `self_hash`.
    fn run_update(
        snapshot: &Arc<DataCache>,
        script: Vec<u8>,
        self_hash: UInt160,
    ) -> (VmState, ApplicationEngine) {
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let mut engine = engine_for(Arc::clone(snapshot), ProtocolSettings::default(), sender);
        engine
            .load_script(script, CallFlags::ALL, Some(self_hash))
            .expect("script loads");
        let state = engine.execute_allow_fault();
        (state, engine)
    }

    #[test]
    fn deploy_writes_record_and_index_charges_fee_and_notifies() {
        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let nef = minimal_nef();
        let manifest = deployable_manifest("DeployFixture");

        let (state, engine) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_json(&manifest),
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::HALT, "deploy must HALT");

        // The record lands at GetContractHash(sender, nef.CheckSum, name) and
        // round-trips through the shared reader.
        let expected_hash = Helper::get_contract_hash(&sender, nef.checksum, "DeployFixture");
        let deployed = ContractManagement::get_contract_from_snapshot(&snapshot, &expected_hash)
            .unwrap()
            .expect("deployed record exists");
        assert_eq!(
            deployed.id, 1,
            "first user contract takes the genesis next-id"
        );
        assert_eq!(deployed.update_counter, 0);
        assert_eq!(deployed.hash, expected_hash);
        assert_eq!(deployed.nef.checksum, nef.checksum);
        assert_eq!(deployed.manifest.name, "DeployFixture");

        // The big-endian id -> hash index entry.
        let index = snapshot
            .get(&ContractManagement::contract_id_storage_key(1))
            .expect("id index entry written");
        assert_eq!(
            index.value_bytes().to_vec(),
            expected_hash.to_bytes().to_vec()
        );
        // The next-available-id counter advanced to 2.
        assert_eq!(
            crate::read_storage_int(
                &snapshot,
                ContractManagement::ID,
                PREFIX_NEXT_AVAILABLE_ID,
                DEFAULT_NEXT_AVAILABLE_ID
            )
            .unwrap(),
            2
        );

        // The 10-GAS minimum deployment fee dominates this tiny payload and
        // was charged (C# AddFee(max(StoragePrice * size, MinimumFee))).
        assert!(
            engine.fee_consumed() >= DEFAULT_MINIMUM_DEPLOYMENT_FEE,
            "deployment fee charged: {}",
            engine.fee_consumed()
        );

        // The Deploy notification carries the new hash.
        let notifications = engine.notifications();
        let deploy_event = notifications
            .iter()
            .find(|n| n.event_name == "Deploy")
            .expect("Deploy event emitted");
        assert_eq!(deploy_event.script_hash, ContractManagement::script_hash());
        assert_eq!(
            deploy_event.state[0].as_bytes().unwrap(),
            expected_hash.to_bytes().to_vec()
        );

        // deploy returns the new ContractState as the 5-field Array.
        let result = engine.result_stack().peek(0).expect("deploy result");
        let StackItem::Array(items) = result else {
            panic!("deploy must return an Array, got {result:?}");
        };
        assert_eq!(items.items().len(), 5);
        assert_eq!(
            items.items()[2].as_bytes().unwrap(),
            expected_hash.to_bytes().to_vec(),
            "field 2 is the contract hash"
        );
    }

    #[test]
    fn deploy_hash_is_deterministic_and_duplicates_fault() {
        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let nef = minimal_nef();
        let manifest = deployable_manifest("DeterministicFixture");
        let manifest_bytes = manifest_json(&manifest);

        let (first, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(first, VmState::HALT);

        // Same sender + NEF checksum + name -> the same hash, so the second
        // deploy hits "Contract Already Exists" and faults.
        let (duplicate, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(duplicate, VmState::FAULT, "duplicate deploy must fault");

        // A different manifest NAME moves the hash: deploys fresh with id 2.
        let renamed = deployable_manifest("DeterministicFixtureB");
        let (second, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_json(&renamed),
            None,
            CallFlags::ALL,
        );
        assert_eq!(second, VmState::HALT);
        let hash_a = Helper::get_contract_hash(&sender, nef.checksum, "DeterministicFixture");
        let hash_b = Helper::get_contract_hash(&sender, nef.checksum, "DeterministicFixtureB");
        assert_ne!(hash_a, hash_b);
        let second_state = ContractManagement::get_contract_from_snapshot(&snapshot, &hash_b)
            .unwrap()
            .expect("second contract deployed");
        assert_eq!(second_state.id, 2, "ids allocate sequentially");
    }

    #[test]
    fn deploy_runs_the_declared_deploy_callback_with_data() {
        // The contract script: `main()` = RET at 0; `_deploy(data, update)` at
        // `deploy_offset` stores [0xEE] under key [0x77] in the contract's own
        // storage — observable proof the queued callback executed.
        let mut script = ScriptBuilder::new();
        script.emit_opcode(OpCode::RET);
        let deploy_offset = script.len() as i32;
        script.emit_instruction(OpCode::INITSLOT, &[0x00, 0x02]);
        script.emit_push(&[0xEE]); // value (deepest)
        script.emit_push(&[0x77]); // key
        script
            .emit_syscall("System.Storage.GetContext")
            .expect("GetContext");
        script.emit_syscall("System.Storage.Put").expect("Put");
        script.emit_opcode(OpCode::RET);
        let nef = NefFile::new("e2e-test".to_string(), script.to_array());

        let mut manifest = deployable_manifest("CallbackFixture");
        manifest.abi.methods.push(
            ContractMethodDescriptor::new(
                "_deploy".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "data".to_string(),
                        ContractParameterType::Any,
                    )
                    .unwrap(),
                    ContractParameterDefinition::new(
                        "update".to_string(),
                        ContractParameterType::Boolean,
                    )
                    .unwrap(),
                ],
                ContractParameterType::Void,
                deploy_offset,
                false,
            )
            .expect("_deploy descriptor"),
        );

        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let (state, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_json(&manifest),
            Some(&[0xAB]), // deploy(nef, manifest, data) overload
            CallFlags::ALL,
        );
        assert_eq!(
            state,
            VmState::HALT,
            "deploy with _deploy callback must HALT"
        );

        // The callback wrote into the new contract's storage space (id 1).
        let row = snapshot
            .get(&StorageKey::new(1, vec![0x77]))
            .expect("_deploy callback wrote the marker row");
        assert_eq!(row.value_bytes().to_vec(), vec![0xEE]);
    }

    #[test]
    fn deploy_callback_local_storage_syscalls_use_csharp_parameter_order() {
        // HF_Faun local storage syscalls follow the same reflection binder order
        // as C#: parameter 0 is on top of the stack. Local.Put(key, value) must
        // pop key before value; Local.Find(prefix, options) must pop prefix
        // before options.
        let mut script = ScriptBuilder::new();
        script.emit_opcode(OpCode::RET);
        let deploy_offset = script.len() as i32;
        script.emit_instruction(OpCode::INITSLOT, &[0x00, 0x02]);
        script.emit_push(&[0xEE]); // value (deeper)
        script.emit_push(&[0x77]); // key (top)
        script
            .emit_syscall("System.Storage.Local.Put")
            .expect("Local.Put");
        script.emit_push_int(0); // options (deeper)
        script.emit_push(&[0x77]); // prefix (top)
        script
            .emit_syscall("System.Storage.Local.Find")
            .expect("Local.Find");
        script.emit_opcode(OpCode::DROP);
        script.emit_opcode(OpCode::RET);
        let nef = NefFile::new("e2e-test".to_string(), script.to_array());

        let mut manifest = deployable_manifest("LocalStorageCallbackFixture");
        manifest.abi.methods.push(
            ContractMethodDescriptor::new(
                "_deploy".to_string(),
                vec![
                    ContractParameterDefinition::new(
                        "data".to_string(),
                        ContractParameterType::Any,
                    )
                    .unwrap(),
                    ContractParameterDefinition::new(
                        "update".to_string(),
                        ContractParameterType::Boolean,
                    )
                    .unwrap(),
                ],
                ContractParameterType::Void,
                deploy_offset,
                false,
            )
            .expect("_deploy descriptor"),
        );

        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let (state, _) = run_deploy(
            &snapshot,
            faun_from_genesis_settings(),
            sender,
            &nef.to_bytes(),
            &manifest_json(&manifest),
            Some(&[0xAB]),
            CallFlags::ALL,
        );
        assert_eq!(
            state,
            VmState::HALT,
            "local storage callback must follow C# syscall parameter order"
        );

        let row = snapshot
            .get(&StorageKey::new(1, vec![0x77]))
            .expect("Local.Put wrote under the key argument");
        assert_eq!(row.value_bytes().to_vec(), vec![0xEE]);
        assert!(
            snapshot.get(&StorageKey::new(1, vec![0xEE])).is_none(),
            "Local.Put must not swap key and value"
        );
    }

    #[test]
    fn deploy_skips_the_callback_when_not_declared() {
        // The minimal fixture declares no `_deploy`: C# OnDeployAsync skips
        // the call (md is null) but still emits Deploy. Nothing is written
        // into the new contract's storage space.
        let snapshot = seeded_snapshot();
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let (state, engine) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &minimal_nef().to_bytes(),
            &manifest_json(&deployable_manifest("NoCallback")),
            Some(&[0xAB]),
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::HALT);
        assert!(
            engine
                .notifications()
                .iter()
                .any(|n| n.event_name == "Deploy")
        );
        let contract_rows: Vec<_> = snapshot
            .find(
                Some(&StorageKey::new(1, Vec::new())),
                SeekDirection::Forward,
            )
            .collect();
        assert!(
            contract_rows.is_empty(),
            "no _deploy, no contract storage writes"
        );
    }

    #[test]
    fn deploy_validation_failures_fault() {
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let nef = minimal_nef();
        let manifest_bytes = manifest_json(&deployable_manifest("FaultFixture"));

        // Empty NEF payload.
        let (state, _) = run_deploy(
            &seeded_snapshot(),
            ProtocolSettings::default(),
            sender,
            &[],
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::FAULT, "empty NEF must fault");

        // Empty manifest payload.
        let (state, _) = run_deploy(
            &seeded_snapshot(),
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &[],
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::FAULT, "empty manifest must fault");

        // A corrupted NEF checksum.
        let mut corrupted = nef.to_bytes();
        let last = corrupted.len() - 1;
        corrupted[last] ^= 0xFF;
        let (state, _) = run_deploy(
            &seeded_snapshot(),
            ProtocolSettings::default(),
            sender,
            &corrupted,
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::FAULT, "bad NEF checksum must fault");

        // The target hash is Policy-blocked (C# "has been blocked").
        let snapshot = seeded_snapshot();
        let blocked_hash = Helper::get_contract_hash(&sender, nef.checksum, "FaultFixture");
        snapshot.add(
            crate::PolicyContract::blocked_account_key(&blocked_hash),
            StorageItem::from_bytes(Vec::new()),
        );
        let (state, _) = run_deploy(
            &snapshot,
            ProtocolSettings::default(),
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(state, VmState::FAULT, "blocked target hash must fault");
        assert!(
            ContractManagement::get_contract_from_snapshot(&snapshot, &blocked_hash)
                .unwrap()
                .is_none(),
            "no record written for a blocked deploy"
        );
    }

    #[test]
    fn deploy_post_aspidochelone_requires_call_flags_all() {
        // Schedule HF_Aspidochelone from genesis: a deploy carrying only
        // States|AllowNotify (the method's minimum) must fault, while
        // CallFlags.All succeeds (C# refs #2653 / #2673).
        let mut settings = ProtocolSettings::default();
        settings.hardforks.insert(Hardfork::HfAspidochelone, 0);
        let sender = UInt160::from_bytes(&SENDER).unwrap();
        let nef = minimal_nef();
        let manifest_bytes = manifest_json(&deployable_manifest("AspidoFixture"));

        let (restricted, _) = run_deploy(
            &seeded_snapshot(),
            settings.clone(),
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::STATES | CallFlags::ALLOW_NOTIFY,
        );
        assert_eq!(
            restricted,
            VmState::FAULT,
            "partial flags must fault post-Aspidochelone"
        );

        let (full, _) = run_deploy(
            &seeded_snapshot(),
            settings,
            sender,
            &nef.to_bytes(),
            &manifest_bytes,
            None,
            CallFlags::ALL,
        );
        assert_eq!(
            full,
            VmState::HALT,
            "CallFlags.All deploy succeeds post-Aspidochelone"
        );
    }

    #[test]
    fn update_bumps_counter_swaps_payloads_and_notifies() {
        crate::install();
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );

        // The entry script IS the updating contract: pin its hash and seed its
        // record (id 7) plus the id index entry.
        let new_nef = NefFile::new("updated-compiler".to_string(), vec![OpCode::RET.byte()]);
        let new_manifest = deployable_manifest("SelfUpdateFixture");
        let script = update_script(
            Some(&new_nef.to_bytes()),
            Some(&manifest_json(&new_manifest)),
            CallFlags::ALL,
        );
        let self_hash = UInt160::from_script(&script);
        let fixture = ContractState::new(
            7,
            self_hash,
            minimal_nef(),
            deployable_manifest("SelfUpdateFixture"),
        );
        put_contract_record(&cache, &fixture);
        let index_key = ContractManagement::contract_id_storage_key(7);
        cache.add(
            index_key.clone(),
            StorageItem::from_bytes(self_hash.to_bytes().to_vec()),
        );
        let snapshot = Arc::new(cache);

        let (state, engine) = run_update(&snapshot, script, self_hash);
        assert_eq!(state, VmState::HALT, "update must HALT");

        // Same id + hash, UpdateCounter bumped, NEF and manifest swapped.
        let updated = ContractManagement::get_contract_from_snapshot(&snapshot, &self_hash)
            .unwrap()
            .expect("updated record exists");
        assert_eq!(updated.id, 7, "id is preserved");
        assert_eq!(updated.hash, self_hash, "hash is preserved");
        assert_eq!(updated.update_counter, 1, "UpdateCounter bumped");
        assert_eq!(updated.nef.compiler, "updated-compiler");
        assert_eq!(updated.nef.checksum, new_nef.checksum);
        assert_eq!(updated.manifest.name, "SelfUpdateFixture");
        // The id index entry is untouched.
        assert_eq!(
            snapshot
                .get(&index_key)
                .expect("index intact")
                .value_bytes()
                .to_vec(),
            self_hash.to_bytes().to_vec()
        );

        // The storage fee on the payload was charged (no minimum-fee floor).
        let payload_len = (new_nef.to_bytes().len() + manifest_json(&new_manifest).len()) as i64;
        assert!(engine.fee_consumed() >= i64::from(engine.storage_price()) * payload_len);

        // The Update notification carries the contract hash.
        let notifications = engine.notifications();
        let update_event = notifications
            .iter()
            .find(|n| n.event_name == "Update")
            .expect("Update event emitted");
        assert_eq!(update_event.script_hash, ContractManagement::script_hash());
        assert_eq!(
            update_event.state[0].as_bytes().unwrap(),
            self_hash.to_bytes().to_vec()
        );
    }

    #[test]
    fn update_with_null_nef_keeps_the_old_nef() {
        crate::install();
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );

        // update(null, manifest): only the manifest changes (one extra
        // supported standard); the NEF stays byte-identical.
        let mut new_manifest = deployable_manifest("NullNefFixture");
        new_manifest.supported_standards = vec!["NEP-17".to_string()];
        let script = update_script(None, Some(&manifest_json(&new_manifest)), CallFlags::ALL);
        let self_hash = UInt160::from_script(&script);
        let original_nef = minimal_nef();
        let fixture = ContractState::new(
            3,
            self_hash,
            original_nef.clone(),
            deployable_manifest("NullNefFixture"),
        );
        put_contract_record(&cache, &fixture);
        let snapshot = Arc::new(cache);

        let (state, _) = run_update(&snapshot, script, self_hash);
        assert_eq!(state, VmState::HALT, "manifest-only update must HALT");
        let updated = ContractManagement::get_contract_from_snapshot(&snapshot, &self_hash)
            .unwrap()
            .expect("record exists");
        assert_eq!(updated.update_counter, 1);
        assert_eq!(updated.nef.checksum, original_nef.checksum, "NEF unchanged");
        assert_eq!(updated.nef.compiler, original_nef.compiler);
        assert_eq!(
            updated.manifest.supported_standards,
            vec!["NEP-17".to_string()]
        );
    }

    #[test]
    fn update_validation_failures_fault() {
        crate::install();

        // Both args null.
        {
            let cache = DataCache::new(false);
            put_contract_record(
                &cache,
                &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
            );
            let script = update_script(None, None, CallFlags::ALL);
            let self_hash = UInt160::from_script(&script);
            put_contract_record(
                &cache,
                &ContractState::new(4, self_hash, minimal_nef(), deployable_manifest("BothNull")),
            );
            let (state, _) = run_update(&Arc::new(cache), script, self_hash);
            assert_eq!(state, VmState::FAULT, "null nef + null manifest must fault");
        }

        // The caller has no contract record.
        {
            let cache = DataCache::new(false);
            put_contract_record(
                &cache,
                &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
            );
            let script = update_script(
                Some(&minimal_nef().to_bytes()),
                Some(&manifest_json(&deployable_manifest("NoRecord"))),
                CallFlags::ALL,
            );
            let self_hash = UInt160::from_script(&script);
            let (state, _) = run_update(&Arc::new(cache), script, self_hash);
            assert_eq!(state, VmState::FAULT, "non-contract caller must fault");
        }

        // The manifest name cannot change.
        {
            let cache = DataCache::new(false);
            put_contract_record(
                &cache,
                &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
            );
            let script = update_script(
                None,
                Some(&manifest_json(&deployable_manifest("RenamedFixture"))),
                CallFlags::ALL,
            );
            let self_hash = UInt160::from_script(&script);
            put_contract_record(
                &cache,
                &ContractState::new(
                    5,
                    self_hash,
                    minimal_nef(),
                    deployable_manifest("OriginalFixture"),
                ),
            );
            let snapshot = Arc::new(cache);
            let (state, _) = run_update(&snapshot, script, self_hash);
            assert_eq!(state, VmState::FAULT, "renaming must fault");
            // The seeded record is untouched (the name check precedes writes).
            let unchanged = ContractManagement::get_contract_from_snapshot(&snapshot, &self_hash)
                .unwrap()
                .expect("record still present");
            assert_eq!(unchanged.manifest.name, "OriginalFixture");
            assert_eq!(unchanged.update_counter, 0);
        }

        // The update counter is saturated at u16::MAX.
        {
            let cache = DataCache::new(false);
            put_contract_record(
                &cache,
                &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
            );
            let script = update_script(
                Some(&minimal_nef().to_bytes()),
                Some(&manifest_json(&deployable_manifest("MaxedFixture"))),
                CallFlags::ALL,
            );
            let self_hash = UInt160::from_script(&script);
            let mut fixture = ContractState::new(
                6,
                self_hash,
                minimal_nef(),
                deployable_manifest("MaxedFixture"),
            );
            fixture.update_counter = u16::MAX;
            put_contract_record(&cache, &fixture);
            let (state, _) = run_update(&Arc::new(cache), script, self_hash);
            assert_eq!(state, VmState::FAULT, "maxed update counter must fault");
        }
    }
}

/// `ContractManagement::initialize` / `ContractManagement::on_persist` against
/// the C# oracle (ContractManagement.cs:53-118): the genesis counter seeds, the
/// native deployment records + `Deploy` notifications, the hardfork manifest
/// refresh (`Update`), and the hardfork-parameterized re-initializations.
#[cfg(test)]
mod persist_tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Arc;

    use neo_config::ProtocolSettings;
    use neo_payloads::{Block, Header};
    use neo_primitives::TriggerType;

    /// C# `PolicyContract.Prefix_ExecFeeFactor`.
    const POLICY_PREFIX_EXEC_FEE_FACTOR: u8 = 18;
    /// C# `PolicyContract.Prefix_BlockedAccount`.
    const POLICY_PREFIX_BLOCKED_ACCOUNT: u8 = 15;
    /// C# `PolicyContract.Prefix_AttributeFee`.
    const POLICY_PREFIX_ATTRIBUTE_FEE: u8 = 20;
    /// C# `Notary.Prefix_MaxNotValidBeforeDelta`.
    const NOTARY_PREFIX_MAX_NOT_VALID_BEFORE_DELTA: u8 = 10;

    fn settings_with(hardforks: &[(Hardfork, u32)]) -> ProtocolSettings {
        ProtocolSettings {
            hardforks: hardforks.iter().copied().collect::<HashMap<_, _>>(),
            ..ProtocolSettings::default()
        }
    }

    fn on_persist_engine(
        snapshot: &Arc<DataCache>,
        settings: &ProtocolSettings,
        index: u32,
        timestamp: u64,
    ) -> ApplicationEngine {
        let mut header = Header::new();
        header.set_index(index);
        header.set_timestamp(timestamp);
        let block = Block::from_parts(header, Vec::new());
        ApplicationEngine::new(
            TriggerType::OnPersist,
            None,
            Arc::clone(snapshot),
            Some(block),
            settings.clone(),
            0,
            None,
        )
        .expect("engine builds")
    }

    fn storage_int(snapshot: &DataCache, id: i32, key: Vec<u8>) -> Option<BigInt> {
        snapshot
            .get(&StorageKey::new(id, key))
            .map(|item| BigInt::from_signed_bytes_le(&item.value_bytes()))
    }

    /// C# `ContractManagement.InitializeAsync` (ContractManagement.cs:53-61):
    /// genesis seeds MinimumDeploymentFee = 10 GAS and NextAvailableId = 1.
    #[test]
    fn initialize_seeds_deployment_fee_and_next_id() {
        let settings = settings_with(&[]);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
        NativeContract::initialize(&ContractManagement::new(), &mut engine).expect("initialize");

        assert_eq!(
            storage_int(
                &snapshot,
                ContractManagement::ID,
                vec![PREFIX_MINIMUM_DEPLOYMENT_FEE],
            ),
            Some(BigInt::from(10_00000000i64))
        );
        assert_eq!(
            storage_int(
                &snapshot,
                ContractManagement::ID,
                vec![PREFIX_NEXT_AVAILABLE_ID],
            ),
            Some(BigInt::from(1))
        );
        // The counter then hands out 1, 2, ... (C# GetNextAvailableId).
        assert_eq!(
            ContractManagement::new()
                .get_next_available_id(&snapshot)
                .unwrap(),
            1
        );
        assert_eq!(
            ContractManagement::new()
                .get_next_available_id(&snapshot)
                .unwrap(),
            2
        );
    }

    /// C# `ContractManagement.OnPersistAsync` at genesis: every genesis-active
    /// native gets a `Prefix_Contract` record (UpdateCounter 0), a
    /// `Prefix_ContractHash` id index entry, and a `Deploy` notification, in
    /// the canonical contract order. Natives activating at an unscheduled
    /// hardfork (Notary/Treasury here) are not deployed (C# IsInitializeBlock
    /// skips unconfigured hardforks).
    #[test]
    fn on_persist_writes_genesis_records_and_deploy_notifications() {
        let settings = settings_with(&[]);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("on_persist");

        let genesis_native_names = [
            "ContractManagement",
            "StdLib",
            "CryptoLib",
            "LedgerContract",
            "NeoToken",
            "GasToken",
            "PolicyContract",
            "RoleManagement",
            "OracleContract",
        ];
        // C# interleaves native initialization with deployment: the
        // genesis-active NEO/GAS initializers emit Transfer before their
        // corresponding Deploy notifications.
        let notifications = engine.notifications();
        assert_eq!(notifications.len(), genesis_native_names.len() + 2);
        assert_eq!(notifications[0].event_name, "Deploy");
        assert_eq!(
            notifications[0].state[0].as_bytes().unwrap(),
            crate::ContractManagement::script_hash().to_bytes()
        );
        assert_eq!(notifications[1].event_name, "Deploy");
        assert_eq!(
            notifications[1].state[0].as_bytes().unwrap(),
            crate::StdLib::script_hash().to_bytes()
        );
        assert_eq!(notifications[2].event_name, "Deploy");
        assert_eq!(
            notifications[2].state[0].as_bytes().unwrap(),
            crate::CryptoLib::script_hash().to_bytes()
        );
        assert_eq!(notifications[3].event_name, "Deploy");
        assert_eq!(
            notifications[3].state[0].as_bytes().unwrap(),
            crate::LedgerContract::script_hash().to_bytes()
        );
        assert_eq!(notifications[4].event_name, "Transfer");
        assert_eq!(notifications[4].script_hash, crate::NeoToken::script_hash());
        assert_eq!(notifications[5].event_name, "Deploy");
        assert_eq!(
            notifications[5].state[0].as_bytes().unwrap(),
            crate::NeoToken::script_hash().to_bytes()
        );
        assert_eq!(notifications[6].event_name, "Transfer");
        assert_eq!(notifications[6].script_hash, crate::GasToken::script_hash());
        assert_eq!(notifications[7].event_name, "Deploy");
        assert_eq!(
            notifications[7].state[0].as_bytes().unwrap(),
            crate::GasToken::script_hash().to_bytes()
        );
        let deploy_notifications = notifications
            .iter()
            .filter(|notification| notification.event_name == "Deploy");
        for (notification, contract) in deploy_notifications.zip(NATIVE_CONTRACTS.iter()) {
            assert_eq!(notification.event_name, "Deploy");
            assert_eq!(notification.script_hash, ContractManagement::script_hash());
            assert_eq!(
                notification.state[0].as_bytes().unwrap(),
                contract.hash().to_bytes(),
                "Deploy order follows the canonical contract order"
            );
        }

        for (contract, name) in NATIVE_CONTRACTS.iter().zip(genesis_native_names.iter()) {
            assert_eq!(contract.name(), *name, "canonical registration order");
            let state = ContractManagement::get_contract_from_snapshot(&snapshot, &contract.hash())
                .unwrap()
                .unwrap_or_else(|| panic!("{name} record missing"));
            assert_eq!(state.id, contract.id());
            assert_eq!(state.hash, contract.hash());
            assert_eq!(state.update_counter, 0);
            assert_eq!(state.manifest.name, *name);
            // The id -> hash index dereferences back to the same record.
            let by_id =
                ContractManagement::get_contract_by_id_from_snapshot(&snapshot, contract.id())
                    .unwrap()
                    .unwrap_or_else(|| panic!("{name} id index missing"));
            assert_eq!(by_id.hash, contract.hash());
        }

        // Unscheduled ActiveIn hardforks: no record, no notification.
        assert!(
            ContractManagement::get_contract_from_snapshot(
                &snapshot,
                &crate::Notary::script_hash()
            )
            .unwrap()
            .is_none()
        );
        assert!(
            ContractManagement::get_contract_from_snapshot(
                &snapshot,
                &crate::Treasury::script_hash()
            )
            .unwrap()
            .is_none()
        );

        // A later non-hardfork block is a complete no-op.
        let mut engine = on_persist_engine(&snapshot, &settings, 1, 1000);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("block 1");
        assert!(engine.notifications().is_empty());
    }

    /// The HF_Echidna activation block (ContractManagement.cs:93-115): natives
    /// whose used hardforks include Echidna get their stored record refreshed
    /// (UpdateCounter++ + the height-composed NEF/manifest) and an `Update`
    /// notification; Notary (ActiveIn = Echidna) is deployed fresh; Policy's
    /// Echidna re-initialization (PolicyContract.cs:144-152) seeds the
    /// NotaryAssisted attribute fee and migrates the block-time settings.
    #[test]
    fn echidna_block_refreshes_manifests_and_runs_policy_reinitialization() {
        let settings = settings_with(&[(Hardfork::HfEchidna, 100)]);
        let snapshot = Arc::new(DataCache::new(false));
        // Genesis deployment pass.
        let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("genesis");

        // Pre-Echidna NEO manifest: NEP-17 only, no onNEP17Payment.
        let neo_hash = crate::NeoToken::script_hash();
        let pre = ContractManagement::get_contract_from_snapshot(&snapshot, &neo_hash)
            .unwrap()
            .unwrap();
        assert_eq!(pre.manifest.supported_standards, ["NEP-17"]);
        assert!(!ContractManagement::abi_has_method(
            &pre.manifest,
            "onNEP17Payment",
            3
        ));

        // The Echidna activation block.
        let mut engine = on_persist_engine(&snapshot, &settings, 100, 100_000);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("echidna block");

        // NEO: refreshed in place — UpdateCounter 1, NEP-27 joins, the Echidna
        // ABI method appears, id/hash unchanged.
        let post = ContractManagement::get_contract_from_snapshot(&snapshot, &neo_hash)
            .unwrap()
            .unwrap();
        assert_eq!(post.update_counter, 1);
        assert_eq!(post.id, crate::NeoToken::ID);
        assert_eq!(post.manifest.supported_standards, ["NEP-17", "NEP-27"]);
        assert!(ContractManagement::abi_has_method(
            &post.manifest,
            "onNEP17Payment",
            3
        ));

        // Notary: deployed fresh at its activation block.
        let notary = ContractManagement::get_contract_from_snapshot(
            &snapshot,
            &crate::Notary::script_hash(),
        )
        .unwrap()
        .expect("Notary deploys at Echidna");
        assert_eq!(notary.update_counter, 0);

        // GAS carries no Echidna-gated metadata: untouched, no notification.
        let gas = ContractManagement::get_contract_from_snapshot(
            &snapshot,
            &crate::GasToken::script_hash(),
        )
        .unwrap()
        .unwrap();
        assert_eq!(gas.update_counter, 0);
        let gas_hash_bytes = crate::GasToken::script_hash().to_bytes();
        assert!(
            engine
                .notifications()
                .iter()
                .all(|n| n.state[0].as_bytes().unwrap() != gas_hash_bytes)
        );

        // Notification kinds: Update for refreshed natives, Deploy for Notary.
        let kinds: HashMap<Vec<u8>, String> = engine
            .notifications()
            .iter()
            .map(|n| {
                (
                    n.state[0].as_bytes().unwrap().to_vec(),
                    n.event_name.clone(),
                )
            })
            .collect();
        assert_eq!(
            kinds.get(&neo_hash.to_bytes().to_vec()),
            Some(&"Update".to_string())
        );
        assert_eq!(
            kinds.get(&crate::Notary::script_hash().to_bytes().to_vec()),
            Some(&"Deploy".to_string())
        );

        // Policy Echidna re-initialization (PolicyContract.cs:144-152).
        let policy_id = crate::PolicyContract::ID;
        assert_eq!(
            storage_int(
                &snapshot,
                policy_id,
                vec![
                    POLICY_PREFIX_ATTRIBUTE_FEE,
                    neo_primitives::TransactionAttributeType::NotaryAssisted.to_byte()
                ]
            ),
            Some(BigInt::from(1000_0000i64)),
            "DefaultNotaryAssistedAttributeFee"
        );
        assert_eq!(
            storage_int(&snapshot, policy_id, vec![21]),
            Some(BigInt::from(settings.milliseconds_per_block)),
            "MillisecondsPerBlock migrates from ProtocolSettings"
        );
        assert_eq!(
            storage_int(&snapshot, policy_id, vec![22]),
            Some(BigInt::from(settings.max_valid_until_block_increment)),
            "MaxValidUntilBlockIncrement migrates from ProtocolSettings"
        );
        assert_eq!(
            storage_int(&snapshot, policy_id, vec![23]),
            Some(BigInt::from(settings.max_traceable_blocks)),
            "MaxTraceableBlocks migrates from ProtocolSettings"
        );

        // Notary's own ActiveIn seeding runs inside ContractManagement
        // OnPersist, matching C# InitializeAsync(HF_Echidna).
        let notary_initialize_seed = storage_int(
            &snapshot,
            crate::Notary::ID,
            vec![NOTARY_PREFIX_MAX_NOT_VALID_BEFORE_DELTA],
        );
        assert_eq!(notary_initialize_seed, Some(BigInt::from(140)));
    }

    /// The HF_Faun activation block: Policy's Faun re-initialization
    /// (PolicyContract.cs:154-168) converts the stored exec-fee factor to
    /// pico-GAS units and stamps blocked accounts with the persisting block's
    /// timestamp; Treasury (ActiveIn = Faun) deploys.
    #[test]
    fn faun_block_reinitializes_policy_and_deploys_treasury() {
        let settings = settings_with(&[(Hardfork::HfFaun, 50)]);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = on_persist_engine(&snapshot, &settings, 0, 0);
        // Genesis: Policy's ActiveIn seeds (the pipeline's initialize pass) +
        // the deployment records.
        NativeContract::initialize(&crate::PolicyContract::new(), &mut engine)
            .expect("policy init");
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("genesis");
        // A pre-Faun blocked account (empty-bytes record).
        let blocked = UInt160::from_bytes(&[0x77; 20]).unwrap();
        let blocked_key = StorageKey::create_with_uint160(
            crate::PolicyContract::ID,
            POLICY_PREFIX_BLOCKED_ACCOUNT,
            &blocked,
        );
        snapshot.add(blocked_key.clone(), StorageItem::from_bytes(Vec::new()));

        let timestamp: u64 = 1_700_000_000_123;
        let mut engine = on_persist_engine(&snapshot, &settings, 50, timestamp);
        NativeContract::on_persist(&ContractManagement::new(), &mut engine).expect("faun block");

        // ExecFeeFactor: 30 datoshi -> 300000 pico-GAS units.
        assert_eq!(
            storage_int(
                &snapshot,
                crate::PolicyContract::ID,
                vec![POLICY_PREFIX_EXEC_FEE_FACTOR]
            ),
            Some(BigInt::from(30i64 * 10_000))
        );
        // The blocked account now carries the persisting block's timestamp.
        assert_eq!(
            storage_int(
                &snapshot,
                crate::PolicyContract::ID,
                blocked_key.key().to_vec()
            ),
            Some(BigInt::from(timestamp))
        );
        // Treasury deploys at Faun.
        let treasury = ContractManagement::get_contract_from_snapshot(
            &snapshot,
            &crate::Treasury::script_hash(),
        )
        .unwrap()
        .expect("Treasury deploys at Faun");
        assert_eq!(treasury.update_counter, 0);
        let kinds: HashMap<Vec<u8>, String> = engine
            .notifications()
            .iter()
            .map(|n| {
                (
                    n.state[0].as_bytes().unwrap().to_vec(),
                    n.event_name.clone(),
                )
            })
            .collect();
        assert_eq!(
            kinds.get(&crate::Treasury::script_hash().to_bytes().to_vec()),
            Some(&"Deploy".to_string())
        );
    }

    /// C# PolicyContract.cs:155-157: the Faun exec-fee-factor conversion
    /// requires Policy to have been initialized ("Policy was not initialized").
    #[test]
    fn faun_reinitialization_faults_when_policy_was_never_initialized() {
        let settings = settings_with(&[(Hardfork::HfFaun, 50)]);
        let snapshot = Arc::new(DataCache::new(false));
        let mut engine = on_persist_engine(&snapshot, &settings, 50, 1);
        let result =
            crate::PolicyContract::new().initialize_for_hardfork(&mut engine, Hardfork::HfFaun);
        assert!(result.is_err(), "missing exec-fee factor must fault");
    }
}
