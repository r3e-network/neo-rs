//! # neo-node::node::application
//!
//! Staged application workflow for opening and running the node daemon.
//!
//! ## Boundary
//!
//! This application-layer facade owns lifecycle order. It exposes node
//! operations and hides storage, channel, service, import, and shutdown
//! mechanics in the lower daemon modules.
//!
//! ## Contents
//!
//! - `command`: Validated operator command and runtime opening.
//! - `runtime`: Opened runtime, requested-mode execution, and graceful stop.

mod command;
mod runtime;

pub(in crate::node) use command::NodeCommand;
