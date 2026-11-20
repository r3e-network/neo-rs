//! Oracle request data type.
//!
//! The record is owned by `neo-protocol` and re-exported here so the oracle
//! service engine keeps its local `neo_oracle::OracleRequest` path while sharing
//! a single definition with the oracle native contract.

pub use neo_protocol::OracleRequest;
