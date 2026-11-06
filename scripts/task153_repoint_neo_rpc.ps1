# Task 153: repoint "safe" (pure re-export) neo_core references in neo-rpc.
# Only symbols verified as pure `pub use` chains in neo-core are rewritten.
$ErrorActionPreference = 'Stop'

$root = 'd:\Git\neo-rs\neo-rpc'
$files = Get-ChildItem -Path $root -Recurse -Filter *.rs | Where-Object {
    (Select-String -Path $_.FullName -Pattern 'neo_core' -Quiet)
}

# ---------------------------------------------------------------------------
# 1) Literal (single-line grouped import) rewrites. Applied first so that the
#    generic path rules below never see a half-converted group.
#    `@@NL@@` is expanded to the file's dominant newline sequence.
# ---------------------------------------------------------------------------
$literals = [ordered]@{
  'use neo_core::smart_contract::{StorageItem, StorageKey};' =
      'use neo_storage::{StorageItem, StorageKey};'
  'use neo_core::persistence::{StorageItem, StorageKey};' =
      'use neo_storage::{StorageItem, StorageKey};'
  'use neo_core::smart_contract::{CallFlags, StorageItem, StorageKey};' =
      'use neo_primitives::CallFlags;@@NL@@use neo_storage::{StorageItem, StorageKey};'
  'use neo_core::smart_contract::{CallFlags, MethodToken};' =
      'use neo_io::MethodToken;@@NL@@use neo_primitives::CallFlags;'
  'use neo_core::ledger::{RelayResult, VerifyResult};' =
      'use neo_core::ledger::RelayResult;@@NL@@use neo_primitives::VerifyResult;'
  'use neo_core::ledger::{TransactionVerificationContext, VerifyResult};' =
      'use neo_core::ledger::TransactionVerificationContext;@@NL@@use neo_primitives::VerifyResult;'
  'use neo_core::neo_io::{Serializable, SerializableExt};' =
      'use neo_io::{Serializable, SerializableExtensions as SerializableExt};'
  'use neo_core::neo_io::{BinaryWriter, MemoryReader, Serializable, SerializableExt};' =
      'use neo_io::{BinaryWriter, MemoryReader, Serializable, SerializableExtensions as SerializableExt};'
  'use neo_core::smart_contract::{BinarySerializer, ContractManifest, ContractState, NefFile};' =
      'use neo_core::smart_contract::{ContractManifest, ContractState, NefFile};@@NL@@use neo_vm::BinarySerializer;'
  'use neo_core::smart_contract::{ApplicationEngine, TriggerType};' =
      'use neo_core::smart_contract::ApplicationEngine;@@NL@@use neo_primitives::TriggerType;'
  'use neo_core::cryptography::{ECCurve, ECPoint};' =
      'use neo_crypto::{ECCurve, ECPoint};'
  # root-level grouped imports
  'use neo_core::{UInt160, UInt256};' =
      'use neo_primitives::{UInt160, UInt256};'
  'use neo_core::{Verifiable, WitnessScope};' =
      'use neo_primitives::{Verifiable, WitnessScope};'
  'use neo_core::{WitnessRule, WitnessScope};' =
      'use neo_io::witness_rule::WitnessRule;@@NL@@use neo_primitives::WitnessScope;'
  'use neo_core::{NeoSystem, ProtocolSettings};' =
      'use neo_protocol::ProtocolSettings;@@NL@@use neo_system::NeoSystem;'
  'use neo_core::{UInt160, WitnessScope};' =
      'use neo_primitives::{UInt160, WitnessScope};'
  'use neo_core::{ECCurve, ECPoint, WitnessCondition, WitnessRule, WitnessRuleAction};' =
      'use neo_crypto::{ECCurve, ECPoint};@@NL@@use neo_io::witness_rule::{WitnessCondition, WitnessRule, WitnessRuleAction};'
  'use neo_core::{KeyPair, UInt160};' =
      'use neo_core::KeyPair;@@NL@@use neo_primitives::UInt160;'
  'use neo_core::{neo_system::NeoSystem, services::RpcService, wallets::Wallet};' =
      'use neo_core::{services::RpcService, wallets::Wallet};@@NL@@use neo_system::NeoSystem;'
  'use neo_core::{Contract, ECPoint, KeyPair, Signer, Transaction};' =
      'use neo_core::{Contract, KeyPair, Signer, Transaction};@@NL@@use neo_crypto::ECPoint;'
  'use neo_core::{Contract, ECPoint, KeyPair, NativeContract, Transaction};' =
      'use neo_core::{Contract, KeyPair, NativeContract, Transaction};@@NL@@use neo_crypto::ECPoint;'
  'use neo_core::{Block, BlockHeader, Contract, ECCurve, ECPoint, KeyPair, Transaction, Witness};' =
      'use neo_core::{Block, BlockHeader, Contract, KeyPair, Transaction, Witness};@@NL@@use neo_crypto::{ECCurve, ECPoint};'
  'use neo_core::{Signer, TransactionAttribute, WitnessCondition};' =
      'use neo_core::{Signer, TransactionAttribute};@@NL@@use neo_io::witness_rule::WitnessCondition;'
  'use neo_core::{UInt160, UInt256, Witness as LedgerWitness};' =
      'use neo_core::Witness as LedgerWitness;@@NL@@use neo_primitives::{UInt160, UInt256};'
  'use neo_core::{NeoSystem, UInt160, UInt256, Verifiable, Witness as LedgerWitness, WitnessScope};' =
      'use neo_core::Witness as LedgerWitness;@@NL@@use neo_primitives::{UInt160, UInt256, Verifiable, WitnessScope};@@NL@@use neo_system::NeoSystem;'
  'use neo_core::{NeoSystem, ScriptBuilder, UInt160, UInt256, Verifiable, WitnessData, WitnessScope};' =
      'use neo_core::WitnessData;@@NL@@use neo_primitives::{UInt160, UInt256, Verifiable, WitnessScope};@@NL@@use neo_system::NeoSystem;@@NL@@use neo_vm::ScriptBuilder;'
  'use neo_core::{NeoSystem, ProtocolSettings, UInt160, WitnessScope};' =
      'use neo_primitives::{UInt160, WitnessScope};@@NL@@use neo_protocol::ProtocolSettings;@@NL@@use neo_system::NeoSystem;'
  'use neo_core::{UInt160, UInt256, Witness as LedgerWitness, WitnessScope};' =
      'use neo_core::Witness as LedgerWitness;@@NL@@use neo_primitives::{UInt160, UInt256, WitnessScope};'
}

# ---------------------------------------------------------------------------
# 2) Regex path rewrites. `\b` guards keep prefixes such as
#    `neo_core::smart_contract::StorageItemExt` / `neo_core::VerifiableExt`
#    untouched (word chars on both sides => no boundary => no match).
# ---------------------------------------------------------------------------
$regexes = @(
  # SerializableExt alias must be rewritten before the generic neo_io rule.
  @('neo_core::neo_io::SerializableExt\b', 'neo_io::SerializableExtensions as SerializableExt'),
  @('neo_core::extensions::SerializableExtensions\b', 'neo_io::SerializableExtensions'),

  # whole-module re-exports
  @('neo_core::cryptography::', 'neo_crypto::'),
  @('neo_core::neo_io::', 'neo_io::'),
  @('neo_core::neo_system::', 'neo_system::'),
  @('neo_core::hardfork::', 'neo_protocol::hardfork::'),
  @('neo_core::witness_rule::', 'neo_io::witness_rule::'),
  @('neo_core::script_validation::', 'neo_vm::'),

  # ProtocolSettings (canonical owner: neo-protocol). neo_core::config::
  # ProtocolSettings is the *distinct* neo-config struct and is left alone.
  @('neo_core::protocol_settings::ProtocolSettings\b', 'neo_protocol::ProtocolSettings'),

  # single symbols
  @('neo_core::rpc::RpcException\b', 'neo_primitives::RpcException'),
  @('neo_core::ledger::VerifyResult\b', 'neo_primitives::VerifyResult'),
  @('neo_core::prelude::Serializable\b', 'neo_io::Serializable'),

  # persistence -> neo-storage
  @('neo_core::persistence::apply_tracked_items\b', 'neo_storage::persistence::transaction::apply_tracked_items'),
  @('neo_core::persistence::providers::MemoryStoreProvider\b', 'neo_storage::persistence::providers::MemoryStoreProvider'),
  @('neo_core::persistence::StoreCache\b', 'neo_storage::persistence::StoreCache'),
  @('neo_core::persistence::DataCache\b', 'neo_storage::persistence::DataCache'),
  @('neo_core::persistence::SeekDirection\b', 'neo_storage::persistence::SeekDirection'),
  @('neo_core::persistence::StoreProvider\b', 'neo_storage::persistence::StoreProvider'),
  @('neo_core::persistence::StorageItem\b', 'neo_storage::StorageItem'),
  @('neo_core::persistence::StorageKey\b', 'neo_storage::StorageKey'),
  @('neo_core::persistence::Store\b', 'neo_storage::persistence::Store'),

  # smart_contract safe subset
  @('neo_core::smart_contract::call_flags::CallFlags\b', 'neo_primitives::CallFlags'),
  @('neo_core::smart_contract::CallFlags\b', 'neo_primitives::CallFlags'),
  @('neo_core::smart_contract::trigger_type::TriggerType\b', 'neo_primitives::TriggerType'),
  @('neo_core::smart_contract::TriggerType\b', 'neo_primitives::TriggerType'),
  @('neo_core::smart_contract::ContractParameterType\b', 'neo_primitives::ContractParameterType'),
  @('neo_core::smart_contract::FindOptions\b', 'neo_primitives::FindOptions'),
  @('neo_core::smart_contract::binary_serializer::', 'neo_vm::binary_serializer::'),
  @('neo_core::smart_contract::BinarySerializer\b', 'neo_vm::BinarySerializer'),
  @('neo_core::smart_contract::MethodToken\b', 'neo_io::MethodToken'),
  @('neo_core::smart_contract::StorageItem\b', 'neo_storage::StorageItem'),
  @('neo_core::smart_contract::StorageKey\b', 'neo_storage::StorageKey'),

  # crate-root aliases
  @('neo_core::ScriptBuilder\b', 'neo_vm::ScriptBuilder'),
  @('neo_core::NeoSystem\b', 'neo_system::NeoSystem'),
  @('neo_core::ProtocolSettings\b', 'neo_protocol::ProtocolSettings'),
  @('neo_core::Hardfork\b', 'neo_protocol::Hardfork'),
  @('neo_core::UInt160\b', 'neo_primitives::UInt160'),
  @('neo_core::UInt256\b', 'neo_primitives::UInt256'),
  @('neo_core::BigDecimal\b', 'neo_primitives::BigDecimal'),
  @('neo_core::WitnessScope\b', 'neo_primitives::WitnessScope'),
  @('neo_core::Verifiable\b', 'neo_primitives::Verifiable'),
  @('neo_core::RpcException\b', 'neo_primitives::RpcException'),
  @('neo_core::ContractParameterType\b', 'neo_primitives::ContractParameterType'),
  @('neo_core::ECCurve\b', 'neo_crypto::ECCurve'),
  @('neo_core::ECPoint\b', 'neo_crypto::ECPoint'),
  @('neo_core::WitnessRuleAction\b', 'neo_io::witness_rule::WitnessRuleAction'),
  @('neo_core::WitnessConditionType\b', 'neo_io::witness_rule::WitnessConditionType'),
  @('neo_core::WitnessCondition\b', 'neo_io::witness_rule::WitnessCondition'),
  @('neo_core::WitnessRule\b', 'neo_io::witness_rule::WitnessRule')
)

$totalHits = 0
$touched = 0
foreach ($f in $files) {
    $text = [System.IO.File]::ReadAllText($f.FullName)
    $orig = $text
    $nl = if ($text.Contains("`r`n")) { "`r`n" } else { "`n" }

    foreach ($k in $literals.Keys) {
        if ($text.Contains($k)) {
            $totalHits += ([regex]::Matches($text, [regex]::Escape($k))).Count
            $text = $text.Replace($k, $literals[$k].Replace('@@NL@@', $nl))
        }
    }
    foreach ($pair in $regexes) {
        $m = [regex]::Matches($text, $pair[0])
        if ($m.Count -gt 0) {
            $totalHits += $m.Count
            $text = [regex]::Replace($text, $pair[0], $pair[1])
        }
    }

    if ($text -ne $orig) {
        $touched++
        [System.IO.File]::WriteAllText($f.FullName, $text)
        Write-Output ("rewrote {0}" -f $f.FullName.Replace('d:\Git\neo-rs\neo-rpc\', ''))
    }
}
Write-Output ("FILES_TOUCHED={0} SITES={1}" -f $touched, $totalHits)
