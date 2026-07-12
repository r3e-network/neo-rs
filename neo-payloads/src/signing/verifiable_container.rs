//! Concrete script-container payloads used during witness/runtime execution.
//!
//! The protocol only installs a small closed set of script containers in the
//! execution engine. Keeping that set as an enum avoids trait-object script
//! containers on the hot runtime path while still implementing the primitive
//! [`neo_primitives::Verifiable`] trait for generic verification helpers.

use std::sync::Arc;

use neo_primitives::{
    UInt256, Verifiable,
    error::{PrimitiveError, PrimitiveResult},
};

use crate::{Block, ExtensiblePayload, Header, Transaction};

/// Hash-only script container used when the caller cannot clone a concrete
/// payload but still needs crypto syscalls to sign `network || hash_data`.
#[derive(Clone, Debug)]
pub struct VerifiableHashContainer {
    hash: UInt256,
    hash_data: Arc<[u8]>,
}

impl VerifiableHashContainer {
    /// Creates a hash-only script container.
    pub fn new(hash: UInt256, hash_data: Vec<u8>) -> Self {
        Self {
            hash,
            hash_data: Arc::from(hash_data.into_boxed_slice()),
        }
    }

    /// Returns the cached container hash.
    pub fn cached_hash(&self) -> UInt256 {
        self.hash
    }
}

/// Validated reference to a transaction owned by an immutable shared block.
///
/// Fields are private so the transaction position can only be created after a
/// bounds check. Holding the block through [`Arc`] keeps the referenced payload
/// stable without a self-referential borrow.
#[derive(Clone, Debug)]
pub struct BlockTransactionContainer {
    block: Arc<Block>,
    index: usize,
}

impl BlockTransactionContainer {
    fn new(block: Arc<Block>, index: usize) -> Option<Self> {
        block.transactions.get(index)?;
        Some(Self { block, index })
    }

    fn transaction(&self) -> &Transaction {
        // `new` validates the position and the block cannot be mutated while
        // this container owns an Arc to it.
        &self.block.transactions[self.index]
    }
}

/// Closed set of Neo script containers observed by `ApplicationEngine`.
#[derive(Clone, Debug)]
pub enum VerifiableContainer {
    /// Transaction script container.
    Transaction(Arc<Transaction>),
    /// Transaction borrowed from an immutable block owned by the container.
    ///
    /// Use [`VerifiableContainer::transaction_in_block`] to construct this
    /// variant. The representation lets block import retain the transaction
    /// without cloning its script, signers, attributes, and witnesses.
    TransactionInBlock(BlockTransactionContainer),
    /// Block script container.
    Block(Arc<Block>),
    /// Header script container.
    Header(Arc<Header>),
    /// Extensible payload script container.
    ExtensiblePayload(Arc<ExtensiblePayload>),
    /// Hash-only fallback container.
    Hash(VerifiableHashContainer),
}

impl VerifiableContainer {
    /// Creates a hash-only fallback container.
    pub fn hash_only(hash: UInt256, hash_data: Vec<u8>) -> Self {
        Self::Hash(VerifiableHashContainer::new(hash, hash_data))
    }

    /// Creates a transaction container borrowing the payload owned by `block`.
    ///
    /// Returns `None` when `index` is outside the block transaction list. The
    /// block is immutable behind [`Arc`], so a validated position remains
    /// valid for the lifetime of the container.
    pub fn transaction_in_block(block: Arc<Block>, index: usize) -> Option<Self> {
        BlockTransactionContainer::new(block, index).map(Self::TransactionInBlock)
    }

    /// Returns the inner transaction when this is a transaction container.
    pub fn as_transaction(&self) -> Option<&Transaction> {
        match self {
            Self::Transaction(tx) => Some(tx.as_ref()),
            Self::TransactionInBlock(transaction) => Some(transaction.transaction()),
            _ => None,
        }
    }

    /// Returns the inner header view when this is a header-like container.
    pub fn as_header(&self) -> Option<&Header> {
        match self {
            Self::Header(header) => Some(header.as_ref()),
            Self::Block(block) => Some(&block.header),
            _ => None,
        }
    }

    /// Returns the inner extensible payload when applicable.
    pub fn as_extensible_payload(&self) -> Option<&ExtensiblePayload> {
        match self {
            Self::ExtensiblePayload(payload) => Some(payload.as_ref()),
            _ => None,
        }
    }
}

impl From<Transaction> for VerifiableContainer {
    fn from(value: Transaction) -> Self {
        Self::Transaction(Arc::new(value))
    }
}

impl From<Arc<Transaction>> for VerifiableContainer {
    fn from(value: Arc<Transaction>) -> Self {
        Self::Transaction(value)
    }
}

impl From<Block> for VerifiableContainer {
    fn from(value: Block) -> Self {
        Self::Block(Arc::new(value))
    }
}

impl From<Arc<Block>> for VerifiableContainer {
    fn from(value: Arc<Block>) -> Self {
        Self::Block(value)
    }
}

impl From<Header> for VerifiableContainer {
    fn from(value: Header) -> Self {
        Self::Header(Arc::new(value))
    }
}

impl From<Arc<Header>> for VerifiableContainer {
    fn from(value: Arc<Header>) -> Self {
        Self::Header(value)
    }
}

impl From<ExtensiblePayload> for VerifiableContainer {
    fn from(value: ExtensiblePayload) -> Self {
        Self::ExtensiblePayload(Arc::new(value))
    }
}

impl From<Arc<ExtensiblePayload>> for VerifiableContainer {
    fn from(value: Arc<ExtensiblePayload>) -> Self {
        Self::ExtensiblePayload(value)
    }
}

impl Verifiable for VerifiableContainer {
    fn verify(&self) -> bool {
        match self {
            Self::Transaction(tx) => tx.verify(),
            Self::TransactionInBlock(transaction) => transaction.transaction().verify(),
            Self::Block(block) => block.verify(),
            Self::Header(header) => header.verify(),
            Self::ExtensiblePayload(payload) => payload.verify(),
            Self::Hash(_) => true,
        }
    }

    fn hash(&self) -> PrimitiveResult<UInt256> {
        match self {
            Self::Transaction(tx) => Verifiable::hash(tx.as_ref()),
            Self::TransactionInBlock(transaction) => Verifiable::hash(transaction.transaction()),
            Self::Block(block) => Verifiable::hash(block.as_ref()),
            Self::Header(header) => Verifiable::hash(header.as_ref()),
            Self::ExtensiblePayload(payload) => Verifiable::hash(payload.as_ref()),
            Self::Hash(container) => Ok(container.cached_hash()),
        }
    }

    fn hash_data(&self) -> Vec<u8> {
        match self {
            Self::Transaction(tx) => tx.hash_data(),
            Self::TransactionInBlock(transaction) => transaction.transaction().hash_data(),
            Self::Block(block) => block.hash_data(),
            Self::Header(header) => header.hash_data(),
            Self::ExtensiblePayload(payload) => payload.hash_data(),
            Self::Hash(container) => container.hash_data.to_vec(),
        }
    }
}

impl TryFrom<&VerifiableContainer> for Transaction {
    type Error = PrimitiveError;

    fn try_from(value: &VerifiableContainer) -> Result<Self, Self::Error> {
        value
            .as_transaction()
            .cloned()
            .ok_or_else(|| PrimitiveError::invalid_data("script container is not a transaction"))
    }
}
