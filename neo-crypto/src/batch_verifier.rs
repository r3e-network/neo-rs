//! Signature verification helper for Neo N3 blockchain
//!
//! Collects ECDSA verification requests and checks them one at a time, caching
//! the aggregate result so repeated queries do not re-verify.

use crate::error::CryptoResult;
use crate::signature::Secp256r1Crypto;

/// Components needed for a single signature verification
#[derive(Debug, Clone)]
pub struct SignatureComponents {
    /// 32-byte message digest (pre-hashed with SHA-256)
    pub message_digest: [u8; 32],

    /// 64-byte ECDSA signature
    pub signature: [u8; 64],

    /// 33-byte compressed public key
    pub public_key: [u8; 33],
}

/// Sequential signature verifier with result caching.
///
/// Verifies signatures individually — does not perform cryptographic
/// batch/aggregate verification. Despite the name, there is no multi-scalar
/// multiplication or random-linear-combination trick here: each added entry is
/// checked with its own `Secp256r1Crypto::verify_prehash` call and the loop
/// stops at the first failure. The only saving is the cached verdict, so a
/// repeated `verify_all` on an unchanged batch does not redo the work.
pub struct BatchVerifier {
    /// All collected (signature, message, public_key) tuples
    batch: Vec<SignatureComponents>,

    /// Whether we've verified this batch yet
    verified: bool,

    /// Error if verification failed
    error: Option<String>,
}

impl Default for BatchVerifier {
    fn default() -> Self {
        Self::new()
    }
}

impl BatchVerifier {
    /// Create a new empty batch verifier with pre-allocated capacity
    #[must_use]
    pub fn new() -> Self {
        Self {
            batch: Vec::with_capacity(32), // Optimized for typical block sizes (~2000 TX)
            verified: false,
            error: None,
        }
    }

    /// Add a signature verification to the batch
    ///
    /// # Arguments
    /// * `message_digest` - 32-byte pre-hashed message
    /// * `signature` - 64-byte ECDSA signature  
    /// * `public_key` - 33-byte compressed public key
    pub fn add(&mut self, message_digest: [u8; 32], signature: [u8; 64], public_key: [u8; 33]) {
        self.batch.push(SignatureComponents {
            message_digest,
            signature,
            public_key,
        });
        self.verified = false;
    }

    /// Verify all accumulated signatures
    ///
    /// Each entry is verified independently and sequentially; the scan aborts
    /// on the first failure. The verdict is cached until the batch changes.
    ///
    /// # Returns
    /// * `true` if ALL signatures are valid
    /// * `false` if ANY signature is invalid
    pub fn verify_all(&mut self) -> CryptoResult<bool> {
        if self.batch.is_empty() {
            return Ok(true);
        }

        if self.verified {
            return Ok(self.error.is_none());
        }

        // Sequential verification with early exit on first failure
        let mut all_valid = true;
        for components in &self.batch {
            match Secp256r1Crypto::verify_prehash(
                &components.message_digest,
                &components.signature,
                &components.public_key,
            ) {
                Ok(true) => {}
                Ok(false) => {
                    all_valid = false;
                    break;
                }
                Err(e) => {
                    all_valid = false;
                    self.error = Some(format!("Verification error: {e:?}"));
                    break;
                }
            }
        }

        self.verified = true;

        if !all_valid {
            self.error = Some("One or more signatures failed verification".to_string());
        }

        Ok(all_valid)
    }

    /// Get number of signatures accumulated so far
    #[must_use]
    pub fn len(&self) -> usize {
        self.batch.len()
    }

    /// Check if batch is empty
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }

    /// Reset the batch verifier for reuse
    pub fn reset(&mut self) {
        self.batch.clear();
        self.verified = false;
        self.error = None;
    }
}
