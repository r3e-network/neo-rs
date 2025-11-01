// Copyright (C) 2015-2025 The Neo Project.
//
// model_extensions.rs ports the helper conversions defined in
// Neo.Plugins.RestServer.Extensions.ModelExtensions.cs. The functions
// here translate core runtime types into the REST-facing models while
// preserving the exact semantics of the C# implementation.

use crate::rest_server::models::node::plugin_model::PluginModel;
use crate::rest_server::models::node::protocol_settings_model::ProtocolSettingsModel;
use crate::rest_server::models::node::remote_node_model::RemoteNodeModel;
use hex::encode_upper;
use neo_core::hardfork::Hardfork;
use neo_core::neo_system::ProtocolSettings;
use neo_core::network::p2p::RemoteNodeSnapshot;
use neo_extensions::plugin::PluginInfo;
use std::collections::HashMap;
use std::convert::TryFrom;

impl From<&ProtocolSettings> for ProtocolSettingsModel {
    fn from(settings: &ProtocolSettings) -> Self {
        let hardforks: HashMap<String, u32> = settings
            .hardforks
            .iter()
            .map(|(fork, height)| (hardfork_name(*fork), *height))
            .collect();

        let validators_count_i32 = i32::try_from(settings.validators_count).unwrap_or(i32::MAX);
        let validators_count_usize = usize::try_from(settings.validators_count)
            .unwrap_or_else(|_| settings.standby_committee.len());

        let standby_committee = settings
            .standby_committee
            .iter()
            .map(|point| encode_upper(point.as_bytes()))
            .collect();

        let standby_validators = settings
            .standby_committee
            .iter()
            .take(validators_count_usize)
            .map(|point| encode_upper(point.as_bytes()))
            .collect();

        ProtocolSettingsModel {
            network: settings.network,
            address_version: settings.address_version,
            validators_count: validators_count_i32,
            milliseconds_per_block: settings.milliseconds_per_block,
            max_valid_until_block_increment: settings.max_valid_until_block_increment,
            max_transactions_per_block: settings.max_transactions_per_block,
            memory_pool_max_transactions: settings.memory_pool_max_transactions,
            max_traceable_blocks: settings.max_traceable_blocks,
            initial_gas_distribution: settings.initial_gas_distribution,
            seed_list: settings.seed_list.clone(),
            hardforks,
            standby_validators,
            standby_committee,
        }
    }
}

impl From<&RemoteNodeSnapshot> for RemoteNodeModel {
    fn from(snapshot: &RemoteNodeSnapshot) -> Self {
        RemoteNodeModel::from_snapshot(snapshot)
    }
}

impl From<RemoteNodeSnapshot> for RemoteNodeModel {
    fn from(snapshot: RemoteNodeSnapshot) -> Self {
        RemoteNodeModel::from_snapshot(&snapshot)
    }
}

impl From<&PluginInfo> for PluginModel {
    fn from(info: &PluginInfo) -> Self {
        PluginModel {
            name: info.name.clone(),
            version: info.version.clone(),
            description: info.description.clone(),
        }
    }
}

fn hardfork_name(fork: Hardfork) -> String {
    let name = format!("{fork:?}");
    if let Some(stripped) = name.strip_prefix("Hf") {
        stripped.to_string()
    } else {
        name
    }
}
