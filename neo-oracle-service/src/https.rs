//! HTTPS oracle protocol implementation (parity with Neo.Plugins.OracleService).

mod client;
mod process;
pub mod security;

pub(crate) use client::OracleHttpsProtocol;
