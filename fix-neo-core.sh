#!/bin/bash
# Quick fix for neo_core references

cd /d/Git/neo-rs/neo-system/src

# Replace neo_core references with correct crates
find . -name "*.rs" -type f -exec sed -i \
  -e 's/neo_core::ledger::block::Block/neo_protocol::block::Block/g' \
  -e 's/neo_core::ledger::block_header::BlockHeader/neo_protocol::block_header::BlockHeader/g' \
  -e 's/neo_core::ledger::MemoryPool/neo_mempool::MemoryPool/g' \
  -e 's/neo_core::ledger::blockchain_application_executed::ApplicationExecuted/neo_ledger::ApplicationExecuted/g' \
  -e 's/neo_core::ledger::create_genesis_block/neo_ledger::create_genesis_block/g' \
  -e 's/neo_core::persistence::DataCache/neo_storage::persistence::DataCache/g' \
  -e 's/neo_core::persistence::data_cache::DataCache/neo_storage::persistence::DataCache/g' \
  -e 's/neo_core::smart_contract::StorageKey/neo_storage::types::StorageKey/g' \
  -e 's/neo_core::smart_contract::native::OracleRequest/neo_native_contracts::oracle::OracleRequest/g' \
  -e 's/neo_core::smart_contract::helper::Helper/neo_smart_contract::helper::Helper/g' \
  -e 's/neo_core::smart_contract::native::composition::default_native_services/neo_native_contracts::default_native_services/g' \
  -e 's/neo_core::state_service::ValidatedRootPersisted/crate::state_service::ValidatedRootPersisted/g' \
  -e 's/neo_core::state_service::state_store::StateServiceSettings/crate::state_service::StateServiceSettings/g' \
  -e 's/neo_core::network::u_pn_p::UPnP/neo_p2p::upnp::UPnP/g' \
  -e 's/neo_core::impl_default_via_new!/\/\/ neo_core::impl_default_via_new! - macro removed/g' \
  {} \;

echo "Replacements done!"
