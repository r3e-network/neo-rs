//! HTTPS oracle protocol implementation (parity with Neo.Plugins.OracleService).

mod client;
mod process;
mod security;

pub(crate) use client::OracleHttpsProtocol;
