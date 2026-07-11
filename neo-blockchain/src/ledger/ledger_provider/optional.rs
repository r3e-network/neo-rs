//! Optional cold-provider composition without trait-object dispatch.

use neo_error::CoreResult;
use neo_payloads::{Block, Header, Transaction, TransactionState};
use neo_primitives::{UInt160, UInt256};

use super::{BlockProvider, TransactionStateProvider, TxProvider};

/// Statically dispatched optional provider used by runtime configuration.
///
/// `Disabled` reports clean misses. `Enabled(P)` delegates to the concrete
/// provider, retaining its error semantics and avoiding `dyn` in read paths.
#[derive(Clone, Debug, Default)]
pub enum OptionalLedgerProvider<P> {
    /// No provider is installed.
    #[default]
    Disabled,
    /// Delegate reads to the installed concrete provider.
    Enabled(P),
}

impl<P> OptionalLedgerProvider<P> {
    /// Creates an optional provider from runtime configuration.
    #[must_use]
    pub fn from_option(provider: Option<P>) -> Self {
        provider.map_or(Self::Disabled, Self::Enabled)
    }

    /// Returns whether a concrete provider is installed.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        matches!(self, Self::Enabled(_))
    }
}

impl<P> BlockProvider for OptionalLedgerProvider<P>
where
    P: BlockProvider,
{
    fn block_hash_by_index(&self, index: u32) -> CoreResult<Option<UInt256>> {
        match self {
            Self::Disabled => Ok(None),
            Self::Enabled(provider) => provider.block_hash_by_index(index),
        }
    }

    fn header_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Header>> {
        match self {
            Self::Disabled => Ok(None),
            Self::Enabled(provider) => provider.header_by_hash(hash),
        }
    }

    fn block_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Block>> {
        match self {
            Self::Disabled => Ok(None),
            Self::Enabled(provider) => provider.block_by_hash(hash),
        }
    }
}

impl<P> TxProvider for OptionalLedgerProvider<P>
where
    P: TxProvider,
{
    fn transaction_by_hash(&self, hash: &UInt256) -> CoreResult<Option<Transaction>> {
        match self {
            Self::Disabled => Ok(None),
            Self::Enabled(provider) => provider.transaction_by_hash(hash),
        }
    }
}

impl<P> TransactionStateProvider for OptionalLedgerProvider<P>
where
    P: TransactionStateProvider,
{
    fn transaction_state_by_hash(&self, hash: &UInt256) -> CoreResult<Option<TransactionState>> {
        match self {
            Self::Disabled => Ok(None),
            Self::Enabled(provider) => provider.transaction_state_by_hash(hash),
        }
    }

    fn contains_conflict_hash(
        &self,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> CoreResult<bool> {
        match self {
            Self::Disabled => Ok(false),
            Self::Enabled(provider) => {
                provider.contains_conflict_hash(hash, signers, max_traceable_blocks)
            }
        }
    }
}
