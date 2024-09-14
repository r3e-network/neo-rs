use std::collections::{HashMap, HashSet};
use chrono::DateTime;
use crate::network::capabilities::FullNodeCapability;
use crate::network::payloads::{Block, VersionPayload};
use crate::uint256::UInt256;

pub struct TaskSession {
    pub inv_tasks: HashMap<UInt256, DateTime<chrono::Utc>>,
    pub index_tasks: HashMap<u32, DateTime<chrono::Utc>>,
    pub available_tasks: HashSet<UInt256>,
    pub received_block: HashMap<u32, Block>,
    pub is_full_node: bool,
    pub last_block_index: u32,
    pub mempool_sent: bool,
}

impl TaskSession {
    pub fn new(version: &VersionPayload) -> Self {
        let full_node = version.capabilities.iter()
            .find_map(|cap| cap.as_any().downcast_ref::<FullNodeCapability>());
        
        let is_full_node = full_node.is_some();
        let last_block_index = full_node.map_or(0, |node| node.start_height);

        Self {
            inv_tasks: HashMap::new(),
            index_tasks: HashMap::new(),
            available_tasks: HashSet::new(),
            received_block: HashMap::new(),
            is_full_node,
            last_block_index,
            mempool_sent: false,
        }
    }

    pub fn has_too_many_tasks(&self) -> bool {
        self.inv_tasks.len() + self.index_tasks.len() >= 100
    }
}
