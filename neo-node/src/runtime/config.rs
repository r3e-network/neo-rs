//! Runtime configuration types.

use neo_consensus::ValidatorInfo;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::state_service::state_store::StateServiceSettings;
use neo_mempool::MempoolConfig;
use neo_p2p::P2PConfig;

/// Channel buffer sizes
pub const CHAIN_CHANNEL_SIZE: usize = 256;
pub const CONSENSUS_CHANNEL_SIZE: usize = 128;
pub const P2P_CHANNEL_SIZE: usize = 512;
pub const SHUTDOWN_CHANNEL_SIZE: usize = 8;

/// Runtime configuration
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Network magic number
    pub network_magic: u32,
    /// Protocol version
    pub protocol_version: u32,
    /// Validator index (None if not a validator)
    pub validator_index: Option<u8>,
    /// Validator list
    pub validators: Vec<ValidatorInfo>,
    /// Private key for signing (empty if not a validator)
    pub private_key: Vec<u8>,
    /// P2P configuration
    pub p2p: P2PConfig,
    /// Mempool configuration
    pub mempool: MempoolConfig,
    /// State service settings (None to disable state root calculation)
    pub state_service: Option<StateServiceSettings>,
    /// Protocol settings for block execution
    pub protocol_settings: ProtocolSettings,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            network_magic: 0x4F454E, // "NEO"
            protocol_version: 0,
            validator_index: None,
            validators: Vec::new(),
            private_key: Vec::new(),
            p2p: P2PConfig::default(),
            mempool: MempoolConfig::default(),
            state_service: None, // Disabled by default
            protocol_settings: ProtocolSettings::default(),
        }
    }
}
