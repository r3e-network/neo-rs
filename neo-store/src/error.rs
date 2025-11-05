use thiserror::Error;

/// Errors raised by storage backends.
#[derive(Debug, Error)]
pub enum StoreError {
    #[error("store: column '{0}' not found")]
    MissingColumn(&'static str),

    #[error("store: backend failure: {0}")]
    Backend(String),
}

impl StoreError {
    #[inline]
    pub fn backend(message: impl Into<String>) -> Self {
        StoreError::Backend(message.into())
    }
}
