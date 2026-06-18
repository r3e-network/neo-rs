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
mod tests;
