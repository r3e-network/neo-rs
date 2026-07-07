//! Wallet compatibility error vocabulary.

/// Wallet-layer failure vocabulary mirroring the C# exceptions the RPC
/// server maps onto JSON-RPC errors.
#[derive(Debug)]
pub(crate) enum WalletCompatError {
    /// C# `InvalidOperationException("Insufficient GAS...")` — wallet
    /// balances cannot cover the system + network fees, or a transfer
    /// amount exceeds the wallet balance.
    InsufficientFunds(String),
    /// Any other invalid-operation failure (faulted probe scripts,
    /// missing contracts, …).
    Other(String),
}

impl std::fmt::Display for WalletCompatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InsufficientFunds(msg) | Self::Other(msg) => f.write_str(msg),
        }
    }
}

pub(super) type WalletCompatResult<T> = Result<T, WalletCompatError>;

pub(super) fn core_err(err: neo_error::CoreError) -> WalletCompatError {
    WalletCompatError::Other(err.to_string())
}
