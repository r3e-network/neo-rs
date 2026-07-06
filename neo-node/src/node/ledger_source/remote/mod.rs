//! JSON-RPC-backed block source for remote-ledger mode.

mod client;
mod payload;
mod source;

#[cfg(test)]
mod tests;

pub(in crate::node) use source::RpcLedgerBlockSource;
