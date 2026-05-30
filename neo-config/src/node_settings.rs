//! Node identification and basic P2P settings.

use serde::{Deserialize, Serialize};

/// Node identification and basic settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeSettings {
    /// Node name for identification
    #[serde(default = "default_node_name")]
    pub name: String,

    /// Listen address for P2P connections
    #[serde(default = "default_listen_address")]
    pub listen_address: String,

    /// P2P port
    #[serde(default = "default_p2p_port")]
    pub p2p_port: u16,

    /// User agent string
    #[serde(default = "default_user_agent")]
    pub user_agent: String,
}

fn default_node_name() -> String {
    format!("neo-rs-{}", &uuid::Uuid::new_v4().to_string()[..8])
}

fn default_listen_address() -> String {
    "0.0.0.0".to_string()
}

const fn default_p2p_port() -> u16 {
    10333
}

fn default_user_agent() -> String {
    format!("/neo-rs:{}/", env!("CARGO_PKG_VERSION"))
}

impl Default for NodeSettings {
    fn default() -> Self {
        Self {
            name: default_node_name(),
            listen_address: default_listen_address(),
            p2p_port: default_p2p_port(),
            user_agent: default_user_agent(),
        }
    }
}
