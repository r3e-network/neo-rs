# Task #228 wave 1: migrate 7 native-contract test suites from neo-core/tests
# to neo-native-contracts/tests with import roots repointed to owning crates.
$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$srcDir = Join-Path $root 'neo-core\tests'
$destDir = Join-Path $root 'neo-native-contracts\tests'
New-Item -ItemType Directory -Force $destDir | Out-Null

$files = @(
  'neotoken_parity_tests.rs',
  'gas_token_compliance_tests.rs',
  'policy_contract_tests.rs',
  'token_management_fungible_tests.rs',
  'token_management_nft_tests.rs',
  'notary_contract_tests.rs',
  'native_contract_tests.rs'
)

# Ordered literal replacements (longest/most-specific first).
$map = [System.Collections.ArrayList]@()
[void]$map.Add(@('neo_core::smart_contract::native::composition::default_native_services', 'neo_native_contracts::composition::default_native_services'))
[void]$map.Add(@('neo_core::smart_contract::native::composition::standard_registry', 'neo_native_contracts::composition::standard_registry'))
[void]$map.Add(@('neo_core::smart_contract::native::token_management::TokenManagement', 'neo_native_contracts::TokenManagement'))
[void]$map.Add(@('neo_core::smart_contract::native::', 'neo_native_contracts::'))
[void]$map.Add(@('neo_core::smart_contract::application_engine::ApplicationEngine', 'neo_smart_contract::application_engine::ApplicationEngine'))
[void]$map.Add(@('neo_core::smart_contract::manifest::ContractEventDescriptor', 'neo_smart_contract::manifest::ContractEventDescriptor'))
[void]$map.Add(@('neo_core::smart_contract::contract_state::ContractState', 'neo_smart_contract::contract_state::ContractState'))
[void]$map.Add(@('neo_core::smart_contract::interoperable::Interoperable', 'neo_vm::Interoperable'))
[void]$map.Add(@('neo_core::smart_contract::BinarySerializer', 'neo_vm::BinarySerializer'))
[void]$map.Add(@('neo_core::smart_contract::TriggerType', 'neo_primitives::trigger_type::TriggerType'))
[void]$map.Add(@('use neo_core::smart_contract::{Contract, ContractParameterType, StorageItem, StorageKey};', "use neo_primitives::ContractParameterType;`r`nuse neo_smart_contract::contract::Contract;`r`nuse neo_storage::{StorageItem, StorageKey};"))
[void]$map.Add(@('neo_core::smart_contract::CallFlags', 'neo_primitives::CallFlags'))
[void]$map.Add(@('neo_core::smart_contract::ContractParameterType', 'neo_primitives::ContractParameterType'))
[void]$map.Add(@('neo_core::network::p2p::payloads::block::Block', 'neo_protocol::payloads::Block'))
[void]$map.Add(@('neo_core::network::p2p::payloads::', 'neo_protocol::payloads::'))
[void]$map.Add(@('neo_core::persistence::DataCache', 'neo_storage::persistence::DataCache'))
[void]$map.Add(@('neo_core::persistence::ReadOnlyStoreGeneric', 'neo_storage::persistence::ReadOnlyStoreGeneric'))
[void]$map.Add(@('neo_core::protocol_settings::ProtocolSettings', 'neo_protocol::ProtocolSettings'))
[void]$map.Add(@('neo_core::hardfork::{Hardfork, HardforkManager}', 'neo_protocol::{Hardfork, HardforkManager}'))
[void]$map.Add(@('neo_core::hardfork::HardforkManager', 'neo_protocol::HardforkManager'))
[void]$map.Add(@('neo_core::hardfork::Hardfork', 'neo_protocol::Hardfork'))
[void]$map.Add(@('neo_core::ledger::create_genesis_block', 'neo_native_contracts::create_genesis_block'))
[void]$map.Add(@('neo_core::neo_vm::StackItem', 'neo_vm::StackItem'))
[void]$map.Add(@('neo_core::cryptography::ECPoint', 'neo_crypto::ECPoint'))
[void]$map.Add(@('neo_core::Witness::empty()', 'neo_protocol::payloads::Witness::empty()'))
[void]$map.Add(@('use neo_core::{Result as CoreResult, UInt160, UInt256, Verifiable, WitnessScope};', "use neo_primitives::{UInt160, UInt256, Verifiable, WitnessScope};`r`nuse neo_smart_contract::error::CoreResult;"))
[void]$map.Add(@('use neo_core::UInt160;', 'use neo_primitives::UInt160;'))
[void]$map.Add(@('neo_core::ScriptBuilder', 'neo_vm::ScriptBuilder'))
[void]$map.Add(@('Some(genesis.to_payload_block())', 'Some(genesis.clone())'))

foreach ($f in $files) {
  $src = Join-Path $srcDir $f
  $dst = Join-Path $destDir $f
  $text = [System.IO.File]::ReadAllText($src)
  foreach ($pair in $map) {
    $text = $text.Replace($pair[0], $pair[1])
  }
  $header = "//`r`n// Migrated from neo-core/tests/$f (Task #228 wave 1).`r`n// Test names and bodies preserved verbatim; import roots repointed to their`r`n// owning crates (neo-native-contracts / neo-smart-contract / neo-protocol /`r`n// neo-storage / neo-primitives / neo-vm / neo-crypto / neo-wallets).`r`n// create_genesis_block here returns neo_protocol::payloads::Block directly, so the`r`n// former genesis.to_payload_block() calls became genesis.clone().`r`n//`r`n"
  [System.IO.File]::WriteAllText($dst, $header + $text, [System.Text.UTF8Encoding]::new($false))
  Write-Host "migrated: $f"
}

# Residual scan: any neo_core:: / to_payload_block left behind?
$residual = Select-String -Path (Join-Path $destDir '*.rs') -Pattern 'neo_core::|to_payload_block'
if ($residual) {
  Write-Host 'RESIDUAL neo_core refs:'
  $residual | ForEach-Object { Write-Host "$($_.Filename):$($_.LineNumber): $($_.Line.Trim())" }
} else {
  Write-Host 'OK: no residual neo_core:: references'
}
