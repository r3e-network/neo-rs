use neo_base::AddressVersion;
use neo_crypto::scrypt::ScryptParams;

mod config;
mod http;
mod http_wallet;
mod node;
mod rpc;
mod runtime_snapshot;
mod services;
mod status;
mod wallet;

pub use config::{default_config, ConfigArgs, NodeConfig};
pub use node::{run, DynStore, Node, SharedStore};
pub use status::{
    ConsensusStatus, NodeStatus, StageState, StageStatus, ValidatorConfig, ValidatorDescriptor,
};

pub const DEFAULT_STAGE_STALE_AFTER_MS: u128 = 5_000;
pub(crate) const DEFAULT_ADDRESS_VERSION: AddressVersion = AddressVersion::MAINNET;
pub(crate) const DEFAULT_SCRYPT_PARAMS: ScryptParams = ScryptParams {
    n: 16_384,
    r: 8,
    p: 8,
};

#[cfg(test)]
mod tests;
