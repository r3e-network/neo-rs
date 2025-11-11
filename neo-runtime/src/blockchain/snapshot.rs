use super::BlockSummary;

#[derive(Debug, Clone)]
pub struct BlockchainSnapshot {
    pub height: u64,
    pub last_block: Option<BlockSummary>,
    pub total_transactions: u64,
    pub total_fees: u64,
    pub total_bytes: u64,
    pub recent_blocks: Vec<BlockSummary>,
}
