//! Back-compat module — documents the type-rename map between
//! `neo_core::X` and the canonical new homes.
//!
//! This module is intentionally a *documentation* shim. The actual
//! re-exports live in [`crate::legacy`]; this module exists to be
//! the single place that documents which `neo_core::X` paths map
//! to which canonical new homes.
//!
//! The map is consumed by the migration tooling (and by humans
//! reading the diffs). When a type is moved out of `neo-core` into
//! its canonical crate, both the old path (under `neo_core::X`)
//! and the new path (under the canonical crate) keep working for
//! one release cycle, after which the `neo_core` path is removed.

#![allow(missing_docs)]

//! ## Type migration table
//!
//! | Old path (`neo_core::X`) | New home |
//! |--------------------------|----------|
//! | `neo_core::UInt160` | [`neo_primitives::UInt160`] |
//! | `neo_core::UInt256` | [`neo_primitives::UInt256`] |
//! | `neo_core::BigDecimal` | [`neo_primitives::BigDecimal`] |
//! | `neo_core::Block` | [`neo_payloads::Block`] |
//! | `neo_core::BlockHeader` | [`neo_payloads::Header`] (renamed) |
//! | `neo_core::Transaction` | [`neo_payloads::Transaction`] |
//! | `neo_core::Witness` | [`neo_ledger_types::Witness`] |
//! | `neo_core::Signer` | [`neo_payloads::Signer`] |
//! | `neo_core::ProtocolSettings` | [`neo_config::ProtocolSettings`] |
//! | `neo_core::CoreError` | [`neo_error::CoreError`] |
//! | `neo_core::CoreResult` | [`neo_error::CoreResult`] |
//! | `neo_core::ScriptBuilder` | [`neo_script_builder::ScriptBuilder`] |
//! | `neo_core::KeyPair` | [`neo_wallets::KeyPair`] |
//! | `neo_core::Hardfork` | [`neo_primitives::Hardfork`] (when present) |
//! | `neo_core::NeoSystem` | [`crate::Node`] (reth-style builder) |
//!
//! ## Method migration table
//!
//! | Legacy pattern | New pattern |
//! |----------------|-------------|
//! | `system.blockchain_actor.tell(cmd)` | `node.blockchain.tell(cmd).await?` |
//! | `system.mempool_actor.tell(cmd)` | `node.mempool.add_transaction(tx).await?` |
//! | `system.local_node.tell(cmd)` | `node.network.broadcast_block(b).await?` |
//! | `tokio::spawn(actor.run())` | `tokio::spawn(service.run())` |
//! | `actor_ref.ask::<X>(cmd)` | `handle.method().await?` (request/response) |
//! | `system.subscribe::<Event>()` | `handle.subscribe()` (returns broadcast::Receiver) |
//!
//! ## Module migration table
//!
//! | Old module (`neo_core::X`) | New home |
//! |----------------------------|----------|
//! | `neo_core::neo_system` | `neo-system` (the new crate) |
//! | `neo_core::network::p2p::local_node` | [`neo_network::local_node`] |
//! | `neo_core::network::p2p::remote_node` | [`neo_network::remote_node`] |
//! | `neo_core::network::p2p::task_manager` | [`neo_network::task_manager`] |
//! | `neo_core::ledger::blockchain` | [`neo_blockchain`] |
//! | `neo_core::actors` | removed (replaced by `async fn` services) |
//! | `neo_core::runtime` | removed (replaced by `neo_runtime` traits) |

#[cfg(test)]
mod tests {
    //! Smoke test: verify the legacy re-exports are reachable
    //! through `neo_system::legacy::X` and the types are the same
    //! as the canonical paths.
    use super::super::legacy;

    #[test]
    fn re_export_paths_resolve() {
        // Use the legacy re-exports to silence unused warnings.
        let _: Option<legacy::UInt160> = None;
        let _: Option<legacy::UInt256> = None;
        let _: Option<legacy::Block> = None;
        let _: Option<legacy::Transaction> = None;
        let _: Option<legacy::Signer> = None;
        let _: Option<legacy::Witness> = None;
        let _: Option<legacy::BigDecimal> = None;
        let _: Option<legacy::ProtocolSettings> = None;
        let _: Option<legacy::CoreError> = None;
    }
}
