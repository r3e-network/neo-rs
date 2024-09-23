mod network;

use crate::core::mpt;
use crate::network::bqueue;
use crate::util;

// StateSync represents state sync module.
pub trait StateSync {
    fn add_mpt_nodes(&self, nodes: Vec<Vec<u8>>) -> Result<(), Box<dyn std::error::Error>>;
    fn blockqueuer(&self) -> &dyn bqueue::Blockqueuer;
    fn init(&self, curr_chain_height: u32) -> Result<(), Box<dyn std::error::Error>>;
    fn is_active(&self) -> bool;
    fn is_initialized(&self) -> bool;
    fn get_unknown_mpt_nodes_batch(&self, limit: usize) -> Vec<util::Uint256>;
    fn need_headers(&self) -> bool;
    fn need_mpt_nodes(&self) -> bool;
    fn traverse<F>(&self, root: util::Uint256, process: F) -> Result<(), Box<dyn std::error::Error>>
    where
        F: Fn(mpt::Node, Vec<u8>) -> bool;
}
