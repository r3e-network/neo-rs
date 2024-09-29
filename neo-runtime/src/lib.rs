// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use std::sync::Arc;

use neo_core::blockchain::BlockChain;
use neo_core::contract::NativeContracts;
use neo_core::store::NeoStates;
use neo_core::types::ChainConfig;
use neo_p2p::{LocalNode, MessageHandleV2, P2pConfig};

pub use fee::*;

pub mod fee;

pub struct NeoConfig {
    pub chain_config: ChainConfig,
    pub p2p_config: P2pConfig,
}

pub struct NeoSystem {
    // network: u32,
    chain: Arc<BlockChain>,
    node: LocalNode,
}

impl NeoSystem {
    pub fn new(states: Arc<dyn NeoStates>, config: NeoConfig) -> Self {
        let natives = NativeContracts::new(states.clone());
        Self {
            // network: config.chain_config.network,
            chain: Arc::new(BlockChain::new(states, natives, config.chain_config)),
            node: LocalNode::new(config.p2p_config),
        }
    }

    pub fn run(&self) {
        let handle = MessageHandleV2::new(self.node.p2p_config().clone(), self.node.net_handles());
        let _node = self.node.run(handle);
    }
}
