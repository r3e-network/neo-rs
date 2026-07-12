//! State-provider error vocabulary.

use std::error::Error;
use std::fmt::{Display, Formatter};

use neo_crypto::mpt_trie::MptError;
use neo_primitives::UInt256;

/// Result returned by state-provider factories and views.
pub type StateProviderResult<T> = Result<T, StateProviderError>;

/// Failure produced while selecting or reading a frozen state view.
#[derive(Debug)]
pub struct StateProviderError {
    kind: StateProviderErrorKind,
}

#[derive(Debug)]
enum StateProviderErrorKind {
    /// A historical root was requested while StateService retains only the
    /// current trie version.
    UnsupportedState {
        /// Whether the backing StateService was configured for full history.
        full_state: bool,
        /// Current root visible in the same frozen snapshot.
        current_root: Option<UInt256>,
        /// Root requested by the caller.
        requested_root: UInt256,
    },
    /// MPT lookup, proof, or traversal failure.
    Mpt(MptError),
}

impl StateProviderError {
    pub(super) const fn unsupported_state(
        full_state: bool,
        current_root: Option<UInt256>,
        requested_root: UInt256,
    ) -> Self {
        Self {
            kind: StateProviderErrorKind::UnsupportedState {
                full_state,
                current_root,
                requested_root,
            },
        }
    }

    /// Returns whether a historical root was rejected by pruning policy.
    #[must_use]
    pub const fn is_unsupported_state(&self) -> bool {
        matches!(self.kind, StateProviderErrorKind::UnsupportedState { .. })
    }

    /// Returns the invalid key/range diagnostic for a malformed provider query.
    ///
    /// Consumers can map this to their own argument-error vocabulary without
    /// depending on the MPT implementation's error enum.
    #[must_use]
    pub fn invalid_argument_message(&self) -> Option<&str> {
        match &self.kind {
            StateProviderErrorKind::Mpt(
                MptError::InvalidOperation(message) | MptError::Key(message),
            ) => Some(message),
            StateProviderErrorKind::UnsupportedState { .. } | StateProviderErrorKind::Mpt(_) => {
                None
            }
        }
    }
}

impl Display for StateProviderError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match &self.kind {
            StateProviderErrorKind::UnsupportedState {
                full_state,
                current_root,
                requested_root,
            } => {
                let full_state = if *full_state { "True" } else { "False" };
                let current_root = current_root
                    .map(|root| root.to_string())
                    .unwrap_or_default();
                write!(
                    formatter,
                    "fullState:{full_state},current:{current_root},rootHash:{requested_root}"
                )
            }
            StateProviderErrorKind::Mpt(error) => Display::fmt(error, formatter),
        }
    }
}

impl Error for StateProviderError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            StateProviderErrorKind::Mpt(error) => Some(error),
            StateProviderErrorKind::UnsupportedState { .. } => None,
        }
    }
}

impl From<MptError> for StateProviderError {
    fn from(error: MptError) -> Self {
        Self {
            kind: StateProviderErrorKind::Mpt(error),
        }
    }
}
