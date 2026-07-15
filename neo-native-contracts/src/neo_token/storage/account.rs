//! NEO account-state codecs and balance reads.

use super::*;

impl NeoToken {
    /// Decodes a stored `NeoAccountState` struct into its fields.
    pub(in crate::neo_token) fn decode_neo_account_state(
        value: &[u8],
    ) -> CoreResult<NeoAccountStateView> {
        let decoded = crate::support::codec::decode_stack_item(value, "neo account state")?;
        NeoAccountStateView::from_stack_item(&decoded)
    }

    /// Encodes a `NeoAccountState` (`Struct[Balance, BalanceHeight, VoteTo,
    /// LastGasPerVote]`) — the write counterpart of [`decode_neo_account_state`].
    pub(in crate::neo_token) fn encode_neo_account_state(
        state: &NeoAccountStateView,
    ) -> CoreResult<Vec<u8>> {
        crate::support::codec::encode_storage_struct(state, "neo account state")
    }

    /// C# `GetAccountState`: the stored `NeoAccountState` struct bytes under
    /// `Prefix_Account ++ account`, or `None` when the account has no entry. The
    /// stored value is already the BinarySerializer-encoded struct (balance,
    /// balanceHeight, voteTo, lastGasPerVote), which is exactly the Array/Struct
    /// return shape, so it is returned as-is (the same pattern as
    /// `getDesignatedByRole` / `getContract`).
    pub(in crate::neo_token) fn read_account_state<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> Option<Vec<u8>> {
        let key = Self::account_key(account);
        snapshot
            .get(&key)
            .map(|item| item.value_bytes().into_owned())
    }

    /// Reads the NEO balance from the NEO-specific account state.
    pub(crate) fn balance_of<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
        account: &UInt160,
    ) -> CoreResult<BigInt> {
        let Some(bytes) = self.read_account_state(snapshot, account) else {
            return Ok(BigInt::from(0));
        };
        Ok(Self::decode_neo_account_state(&bytes)?.balance)
    }
}
