//! Hard bounds and construction errors for resolved host-access policies.

/// Hard bounds for one resolved invocation policy.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HostAccessPolicyLimits {
    /// Maximum exact resolved declarations.
    pub max_declarations: usize,
    /// Maximum deterministic retained declaration bytes.
    pub max_bytes: usize,
}

impl HostAccessPolicyLimits {
    /// Conservative bounds above the default candidate-contract maxima.
    pub const DEFAULT: Self = Self {
        max_declarations: 512,
        max_bytes: 1024 * 1024,
    };
}

impl Default for HostAccessPolicyLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Bounded resolved-policy construction failure.
#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum HostAccessPolicyError {
    /// One hard limit is zero.
    #[error("host access policy limit `{limit}` must be non-zero")]
    ZeroLimit {
        /// Invalid limit.
        limit: &'static str,
    },
    /// Declaration count exceeded its bound.
    #[error("host access policy declaration capacity exceeded (maximum {maximum})")]
    DeclarationCapacity {
        /// Configured maximum.
        maximum: usize,
    },
    /// Retained declaration bytes exceeded their bound.
    #[error("host access policy requires {required} bytes, maximum {maximum}")]
    ByteCapacity {
        /// Required deterministic bytes.
        required: usize,
        /// Configured maximum.
        maximum: usize,
    },
}
