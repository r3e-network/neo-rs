//! Fork choice rules for chain selection

use crate::{BlockIndex, BlockIndexEntry, ChainError, ChainResult};
use neo_primitives::UInt256;

/// Fork choice strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForkChoiceStrategy {
    /// Longest chain rule (by height)
    LongestChain,

    /// Heaviest chain rule (by cumulative difficulty)
    HeaviestChain,

    /// Hybrid: height first, then difficulty for ties
    Hybrid,
}

/// Fork choice decision maker
pub struct ForkChoice {
    /// Strategy to use
    strategy: ForkChoiceStrategy,
}

impl ForkChoice {
    /// Create a new fork choice with the given strategy
    pub fn new(strategy: ForkChoiceStrategy) -> Self {
        Self { strategy }
    }

    /// Create with default hybrid strategy
    pub fn default_strategy() -> Self {
        Self::new(ForkChoiceStrategy::Hybrid)
    }

    /// Get the current strategy
    pub fn strategy(&self) -> ForkChoiceStrategy {
        self.strategy
    }

    /// Compare two blocks and determine which should be the chain tip
    ///
    /// Returns true if `candidate` should become the new tip over `current`
    pub fn should_switch(&self, current: &BlockIndexEntry, candidate: &BlockIndexEntry) -> bool {
        match self.strategy {
            ForkChoiceStrategy::LongestChain => candidate.height > current.height,
            ForkChoiceStrategy::HeaviestChain => {
                candidate.cumulative_difficulty > current.cumulative_difficulty
            }
            ForkChoiceStrategy::Hybrid => {
                if candidate.height > current.height {
                    true
                } else if candidate.height == current.height {
                    candidate.cumulative_difficulty > current.cumulative_difficulty
                } else {
                    false
                }
            }
        }
    }

    /// Find the best block among multiple candidates
    pub fn find_best<'a>(&self, candidates: &'a [BlockIndexEntry]) -> Option<&'a BlockIndexEntry> {
        candidates.iter().reduce(|best, candidate| {
            if self.should_switch(best, candidate) {
                candidate
            } else {
                best
            }
        })
    }

    /// Find common ancestor of two chain tips
    pub fn find_common_ancestor(
        index: &BlockIndex,
        hash_a: &UInt256,
        hash_b: &UInt256,
    ) -> ChainResult<UInt256> {
        // Get chains back to genesis (or max depth)
        let chain_a = index.get_chain(hash_a, 10000);
        let chain_b = index.get_chain(hash_b, 10000);

        // Convert to set for O(1) lookup
        let set_a: std::collections::HashSet<_> = chain_a.iter().collect();

        // Find first hash in chain_b that exists in chain_a
        for hash in &chain_b {
            if set_a.contains(hash) {
                return Ok(*hash);
            }
        }

        Err(ChainError::ReorgFailed(
            "No common ancestor found".to_string(),
        ))
    }

    /// Calculate reorg path from current tip to new tip
    ///
    /// Returns (blocks_to_disconnect, blocks_to_connect)
    pub fn calculate_reorg_path(
        index: &BlockIndex,
        current_tip: &UInt256,
        new_tip: &UInt256,
    ) -> ChainResult<(Vec<UInt256>, Vec<UInt256>)> {
        let common_ancestor = Self::find_common_ancestor(index, current_tip, new_tip)?;

        // Blocks to disconnect: current tip back to (but not including) common ancestor
        let mut to_disconnect = Vec::new();
        let current_chain = index.get_chain(current_tip, 10000);
        for hash in current_chain {
            if hash == common_ancestor {
                break;
            }
            to_disconnect.push(hash);
        }

        // Blocks to connect: from common ancestor (exclusive) to new tip
        let mut to_connect = Vec::new();
        let new_chain = index.get_chain(new_tip, 10000);
        for hash in new_chain {
            if hash == common_ancestor {
                break;
            }
            to_connect.push(hash);
        }
        // Reverse so we connect from fork point to new tip
        to_connect.reverse();

        Ok((to_disconnect, to_connect))
    }

    /// Check if a reorganization is needed
    pub fn needs_reorg(&self, current_tip: &BlockIndexEntry, new_block: &BlockIndexEntry) -> bool {
        // If new block doesn't extend current tip, we might need a reorg
        if new_block.prev_hash != current_tip.hash {
            // Only reorg if the new chain would be better
            self.should_switch(current_tip, new_block)
        } else {
            // Direct extension, no reorg needed
            false
        }
    }
}

impl Default for ForkChoice {
    fn default() -> Self {
        Self::default_strategy()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_entry(height: u32, difficulty: u64) -> BlockIndexEntry {
        let mut hash = [0u8; 32];
        hash[0] = height as u8;

        BlockIndexEntry {
            hash: UInt256::from(hash),
            height,
            prev_hash: UInt256::zero(),
            header: Vec::new(),
            timestamp: 1000000 + height as u64 * 15000,
            tx_count: 1,
            size: 100,
            cumulative_difficulty: difficulty,
            on_main_chain: true,
        }
    }

    #[test]
    fn test_longest_chain() {
        let fork = ForkChoice::new(ForkChoiceStrategy::LongestChain);

        let current = create_entry(10, 100);
        let candidate_higher = create_entry(11, 50); // Lower difficulty but higher
        let candidate_lower = create_entry(9, 200); // Higher difficulty but lower

        assert!(fork.should_switch(&current, &candidate_higher));
        assert!(!fork.should_switch(&current, &candidate_lower));
    }

    #[test]
    fn test_heaviest_chain() {
        let fork = ForkChoice::new(ForkChoiceStrategy::HeaviestChain);

        let current = create_entry(10, 100);
        let candidate_heavier = create_entry(9, 150); // Heavier but shorter
        let candidate_lighter = create_entry(11, 50); // Taller but lighter

        assert!(fork.should_switch(&current, &candidate_heavier));
        assert!(!fork.should_switch(&current, &candidate_lighter));
    }

    #[test]
    fn test_hybrid() {
        let fork = ForkChoice::new(ForkChoiceStrategy::Hybrid);

        let current = create_entry(10, 100);

        // Height wins
        let taller = create_entry(11, 50);
        assert!(fork.should_switch(&current, &taller));

        // Same height, difficulty wins
        let same_height_heavier = create_entry(10, 150);
        assert!(fork.should_switch(&current, &same_height_heavier));

        // Same height, lighter loses
        let same_height_lighter = create_entry(10, 50);
        assert!(!fork.should_switch(&current, &same_height_lighter));
    }

    #[test]
    fn test_find_best() {
        let fork = ForkChoice::default();

        let candidates = vec![
            create_entry(5, 50),
            create_entry(10, 100),
            create_entry(8, 80),
        ];

        let best = fork.find_best(&candidates).unwrap();
        assert_eq!(best.height, 10);
    }
}
