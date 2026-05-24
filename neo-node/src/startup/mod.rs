//! Startup orchestration for neo-node.
//!
//! This module contains the node startup sequence: configuration loading,
//! storage selection, service initialization, signal handling, and graceful
//! shutdown.

pub(crate) mod cli;
mod config;
mod logging;
pub(crate) mod services;
mod signal;
mod run;

pub(crate) use config::STORAGE_VERSION;
pub(crate) use run::run;
