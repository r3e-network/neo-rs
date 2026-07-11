use std::borrow::Borrow;
use std::fmt;

use neo_config::ProtocolSettings;
use neo_execution::NativeContract;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::Block;
use neo_primitives::UInt256;

use crate::native_persist::{NativePersistOptions, NativePersistResources};
use crate::service_context::BlockPersistContext;

use super::types::{EmptyBlockFastForwardPlan, MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS};

/// Reason an empty-block interval cannot be fast-forwarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EmptyBlockFastForwardRejection {
    /// The candidate list is empty.
    EmptyCandidate,
    /// The caller is not trusted bulk-sync persistence.
    NotBulkSync,
    /// Replay artifacts/events are still required for this path.
    ReplayArtifactsEnabled,
    /// The range would exceed [`MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS`].
    BatchTooLarge {
        /// Candidate block count.
        count: usize,
        /// Maximum allowed block count.
        max: usize,
    },
    /// The first block does not directly follow the canonical tip.
    NonNextStart {
        /// Expected first height.
        expected: u32,
        /// Actual first height.
        actual: u32,
    },
    /// A later block is not the next contiguous height.
    NonContiguous {
        /// Expected height.
        expected: u32,
        /// Actual height.
        actual: u32,
    },
    /// A block carries transactions.
    ContainsTransactions {
        /// Block height.
        height: u32,
        /// Number of transactions in the block.
        tx_count: usize,
    },
    /// A block header is not a Neo empty-block header.
    NonEmptyMerkleRoot {
        /// Block height.
        height: u32,
        /// Observed non-zero merkle root.
        merkle_root: UInt256,
    },
    /// A native contract would initialize or refresh at this height.
    NativeInitializationHeight {
        /// Block height.
        height: u32,
        /// Native contract name.
        contract: String,
    },
    /// A currently active native contract has not opted into the fast path.
    NativeContractNotOptedIn {
        /// Block height.
        height: u32,
        /// Native contract name.
        contract: String,
    },
}

impl fmt::Display for EmptyBlockFastForwardRejection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyCandidate => write!(f, "empty-block fast-forward candidate is empty"),
            Self::NotBulkSync => write!(f, "empty-block fast-forward requires bulk sync"),
            Self::ReplayArtifactsEnabled => {
                write!(
                    f,
                    "empty-block fast-forward requires replay artifacts disabled"
                )
            }
            Self::BatchTooLarge { count, max } => write!(
                f,
                "empty-block fast-forward batch too large: {count} > {max}"
            ),
            Self::NonNextStart { expected, actual } => write!(
                f,
                "empty-block fast-forward start is not next height: expected {expected}, got {actual}"
            ),
            Self::NonContiguous { expected, actual } => write!(
                f,
                "empty-block fast-forward range is not contiguous: expected {expected}, got {actual}"
            ),
            Self::ContainsTransactions { height, tx_count } => {
                write!(f, "block {height} has {tx_count} transactions")
            }
            Self::NonEmptyMerkleRoot {
                height,
                merkle_root,
            } => write!(f, "block {height} has non-empty merkle root {merkle_root}"),
            Self::NativeInitializationHeight { height, contract } => write!(
                f,
                "block {height} initializes or refreshes native contract {contract}"
            ),
            Self::NativeContractNotOptedIn { height, contract } => write!(
                f,
                "native contract {contract} is active at {height} but has not opted into empty-block fast-forward"
            ),
        }
    }
}

impl std::error::Error for EmptyBlockFastForwardRejection {}

/// Inputs for [`plan_empty_block_fast_forward`].
pub struct EmptyBlockFastForwardRequest<'a, B, P>
where
    P: NativeContractProvider + 'static,
{
    /// Current canonical chain height before the interval.
    pub current_height: u32,
    /// Candidate blocks, expected to start at `current_height + 1`.
    pub blocks: &'a [B],
    /// Protocol settings used for native activation checks.
    pub settings: &'a ProtocolSettings,
    /// Reusable native persistence resources for this import batch.
    pub resources: &'a NativePersistResources<P>,
    /// Replay artifact policy of the caller.
    pub persist_options: NativePersistOptions,
    /// Persistence context of the caller.
    pub persist_context: BlockPersistContext,
}

/// Validates whether `blocks` may be persisted by a state-equivalent
/// empty-block fast-forward writer.
pub fn plan_empty_block_fast_forward<B, P>(
    request: EmptyBlockFastForwardRequest<'_, B, P>,
) -> Result<EmptyBlockFastForwardPlan, EmptyBlockFastForwardRejection>
where
    B: Borrow<Block>,
    P: NativeContractProvider + 'static,
{
    let EmptyBlockFastForwardRequest {
        current_height,
        blocks,
        settings,
        resources,
        persist_options,
        persist_context,
    } = request;
    if blocks.is_empty() {
        return Err(EmptyBlockFastForwardRejection::EmptyCandidate);
    }
    if !persist_context.is_trusted_replay() {
        return Err(EmptyBlockFastForwardRejection::NotBulkSync);
    }
    if persist_options.capture_replay_artifacts {
        return Err(EmptyBlockFastForwardRejection::ReplayArtifactsEnabled);
    }
    if blocks.len() > MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS {
        return Err(EmptyBlockFastForwardRejection::BatchTooLarge {
            count: blocks.len(),
            max: MAX_EMPTY_BLOCK_FAST_FORWARD_BLOCKS,
        });
    }

    let start = blocks[0].borrow().index();
    let expected_start = current_height.saturating_add(1);
    if start != expected_start {
        return Err(EmptyBlockFastForwardRejection::NonNextStart {
            expected: expected_start,
            actual: start,
        });
    }

    for (offset, block) in blocks.iter().enumerate() {
        let block = block.borrow();
        let expected = start.saturating_add(offset as u32);
        let height = block.index();
        if height != expected {
            return Err(EmptyBlockFastForwardRejection::NonContiguous {
                expected,
                actual: height,
            });
        }
        if !block.transactions.is_empty() {
            return Err(EmptyBlockFastForwardRejection::ContainsTransactions {
                height,
                tx_count: block.transactions.len(),
            });
        }
        if block.header.merkle_root() != &UInt256::zero() {
            return Err(EmptyBlockFastForwardRejection::NonEmptyMerkleRoot {
                height,
                merkle_root: *block.header.merkle_root(),
            });
        }

        for contract in resources.contracts() {
            let (initialize, _hardforks) = contract.is_initialize_block(settings, height);
            if initialize {
                return Err(EmptyBlockFastForwardRejection::NativeInitializationHeight {
                    height,
                    contract: contract.name().to_string(),
                });
            }
            if contract.is_active(settings, height) && !contract.supports_empty_block_fast_forward()
            {
                return Err(EmptyBlockFastForwardRejection::NativeContractNotOptedIn {
                    height,
                    contract: contract.name().to_string(),
                });
            }
        }
    }

    Ok(EmptyBlockFastForwardPlan {
        start,
        end: blocks[blocks.len() - 1].borrow().index(),
        block_count: blocks.len(),
    })
}
