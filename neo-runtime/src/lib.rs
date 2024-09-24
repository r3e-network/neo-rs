// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use neo_core::blockchain::BlockChain;
use neo_p2p::{LocalNode, MessageHandleV2, P2pConfig};

pub mod fee;

pub struct NeoSystem {
    chain: BlockChain,
    node: LocalNode,
}

impl NeoSystem {
    pub fn new() -> Self {
        Self { chain: BlockChain::new(), node: LocalNode::new(P2pConfig::default()) }
    }

    pub fn run(&self) {
        let handle = MessageHandleV2::new(
            self.node.port(),
            self.node.p2p_config().clone(),
            self.node.net_handles(),
        );
        let _node = self.node.run(handle);
    }
}
