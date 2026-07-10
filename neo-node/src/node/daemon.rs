//! High-level daemon entrypoint.
//!
//! The staged application facade owns lifecycle ordering. This module stays at
//! one abstraction level and deliberately does not access service, storage,
//! import, networking, or shutdown mechanics directly.

use clap::Parser;

use super::application::NodeCommand;
use super::cli::NodeCli;

pub(super) async fn run() -> anyhow::Result<()> {
    NodeCommand::from_cli(NodeCli::parse())?
        .open_runtime()
        .await?
        .run_requested_mode()
        .await
}
