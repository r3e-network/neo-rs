//! JSON-RPC-backed block source for remote-ledger mode.

mod client;
mod payload;
mod source;

#[cfg(test)]
#[path = "../../../tests/node/ledger_source/remote.rs"]
mod tests;

pub(in crate::node) use source::RpcLedgerBlockSource;
