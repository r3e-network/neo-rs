/// Result of state root verification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StateRootVerificationResult {
    /// Verification succeeded.
    Valid,
    /// State root mismatch - computed root differs from expected.
    RootMismatch,
    /// State root not found.
    NotFound,
    /// Missing witness for validated root.
    MissingWitness,
    /// Witness verification failed.
    InvalidWitness,
    /// State root index mismatch.
    IndexMismatch,
    /// Verifier not configured.
    VerifierNotConfigured,
}

impl std::fmt::Display for StateRootVerificationResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Valid => write!(f, "valid"),
            Self::RootMismatch => write!(f, "root hash mismatch"),
            Self::NotFound => write!(f, "state root not found"),
            Self::MissingWitness => write!(f, "missing witness"),
            Self::InvalidWitness => write!(f, "invalid witness"),
            Self::IndexMismatch => write!(f, "index mismatch"),
            Self::VerifierNotConfigured => write!(f, "verifier not configured"),
        }
    }
}
