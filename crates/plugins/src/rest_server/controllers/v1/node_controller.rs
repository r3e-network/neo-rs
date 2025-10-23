// Copyright (C) 2015-2025 The Neo Project.
//
// NodeController mirrors Neo.Plugins.RestServer.Controllers.v1.NodeController.
// It exposes node-centric information through the REST surface while relying on
// the shared global state supplied by the RestServer plugin.

use crate::rest_server::models::error::error_model::ErrorModel;
use crate::rest_server::models::node::{PluginModel, ProtocolSettingsModel, RemoteNodeModel};
use crate::rest_server::rest_server_plugin::RestServerGlobals;
use neo_core::NeoSystem;
use neo_core::network::p2p::local_node::LocalNode;
use std::sync::{Arc, RwLock, RwLockReadGuard};

/// Node controller matching C# `Neo.Plugins.RestServer.Controllers.v1.NodeController` behaviour.
#[derive(Debug, Default)]
pub struct NodeController;

impl NodeController {
    /// Creates a new controller instance ensuring the plugin has been initialised properly.
    pub fn new() -> Result<Self, ErrorModel> {
        Self::neo_system()?;
        Self::local_node()?;
        Ok(Self::default())
    }

    /// Gets the connected remote nodes ordered by their reported height.
    pub fn get_peers(&self) -> Result<Vec<RemoteNodeModel>, ErrorModel> {
        let local_node = Self::local_node()?;
        let mut snapshots = local_node.remote_nodes();
        snapshots.sort_by(|a, b| b.last_block_index.cmp(&a.last_block_index));
        Ok(snapshots
            .iter()
            .map(RemoteNodeModel::from_snapshot)
            .collect())
    }

    /// Gets all loaded plugins from the running Neo system.
    pub fn get_plugins(&self) -> Result<Vec<PluginModel>, ErrorModel> {
        let system = Self::neo_system()?;
        let plugin_manager = system.plugin_manager();
        let manager = read_lock(&plugin_manager);
        Ok(manager
            .plugin_infos()
            .into_iter()
            .map(|(name, version)| PluginModel::with_params(name, version, String::new()))
            .collect())
    }

    /// Gets the protocol settings for the current network.
    pub fn get_settings(&self) -> Result<ProtocolSettingsModel, ErrorModel> {
        let system = Self::neo_system()?;
        let settings = system.settings();
        Ok(ProtocolSettingsModel::from(settings))
    }

    fn neo_system() -> Result<Arc<NeoSystem>, ErrorModel> {
        RestServerGlobals::neo_system().ok_or_else(|| {
            error_model(
                1001,
                "InvalidOperation",
                "NeoSystem has not been initialised. Ensure the node is started.",
            )
        })
    }

    fn local_node() -> Result<Arc<LocalNode>, ErrorModel> {
        RestServerGlobals::local_node().ok_or_else(|| {
            error_model(
                1002,
                "InvalidOperation",
                "LocalNode is not available. Verify the network subsystem is running.",
            )
        })
    }
}

fn read_lock<T>(lock: &Arc<RwLock<T>>) -> RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn error_model(code: i32, name: &str, message: &str) -> ErrorModel {
    ErrorModel::with_params(code, name.to_string(), message.to_string())
}
