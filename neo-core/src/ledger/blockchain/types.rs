//
// types.rs - Message types and enums for Blockchain actor
//

use super::*;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct UnverifiedBlocksList {
    pub(super) blocks: Vec<Block>,
    nodes: HashSet<String>,
}

impl UnverifiedBlocksList {
    #[allow(dead_code)]
    pub(super) fn new() -> Self {
        Self {
            blocks: Vec::new(),
            nodes: HashSet::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistCompleted {
    pub block: Block,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub blocks: Vec<Block>,
    pub verify: bool,
}

impl Default for Import {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            verify: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCompleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillMemoryPool {
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillCompleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InventoryPayload {
    Block(Box<Block>),
    Transaction(Box<Transaction>),
    Extensible(Box<ExtensiblePayload>),
    Raw(InventoryType, Vec<u8>),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverifyItem {
    pub payload: InventoryPayload,
    #[serde(default)]
    pub block_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reverify {
    pub inventories: Vec<ReverifyItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImportDisposition {
    AlreadySeen,
    NextExpected,
    FutureGap,
}

pub(super) fn classify_import_block(current_height: u32, block_index: u32) -> ImportDisposition {
    if block_index <= current_height {
        ImportDisposition::AlreadySeen
    } else if block_index == current_height.saturating_add(1) {
        ImportDisposition::NextExpected
    } else {
        ImportDisposition::FutureGap
    }
}

#[cfg(test)]
pub(super) fn should_schedule_reverify_idle(more_pending: bool, header_backlog: bool) -> bool {
    more_pending && !header_backlog
}

pub use crate::ledger::transaction_router::PreverifyCompleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayResult {
    pub hash: UInt256,
    pub inventory_type: InventoryType,
    pub block_index: Option<u32>,
    pub result: VerifyResult,
}

#[derive(Debug, Clone)]
pub enum BlockchainCommand {
    PersistCompleted(PersistCompleted),
    Import(Import),
    FillMemoryPool(FillMemoryPool),
    FillCompleted,
    Reverify(Reverify),
    InventoryBlock {
        block: Block,
        relay: bool,
    },
    InventoryExtensible {
        payload: ExtensiblePayload,
        relay: bool,
    },
    PreverifyCompleted(PreverifyCompleted),
    Headers(Vec<Header>),
    Idle,
    RelayResult(RelayResult),
    Initialize,
    AttachSystem(Arc<NeoSystemContext>),
}
