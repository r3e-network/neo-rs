//! # neo-state-packs store hashing
//!
//! Owns checksums for append frames and authenticated immutable index-run structures.
//!
//! ## Boundary
//!
//! This module hashes already encoded bytes. Frame and index codecs own byte layout, while this
//! module owns the version-specific digest domains and excludes checksum fields from their input.
//!
//! ## Contents
//!
//! - [`digest`]: computes an ordinary SHA-256 checksum.
//! - [`index_structure_digest`]: authenticates one contiguous index structure.
//! - [`index_structure_digest_parts`]: authenticates streamed index structure parts.

use super::{
    INDEX_HEADER_LEN, INDEX_HEADER_TAG_START, INDEX_STRUCTURE_SHA256_END,
    INDEX_STRUCTURE_SHA256_START, PACK_INDEX_FORMAT_VERSION, PackStoreArtifact, PackStoreError,
};
use anyhow::Result;
use sha2::{Digest, Sha256};

const INDEX_STRUCTURE_DIGEST_DOMAIN_V5: &[u8] = b"neo-state-packs/index-structure/v5\0";

/// Computes the SHA-256 checksum of `bytes`.
pub(super) fn digest(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}

/// Authenticates the lookup structure following an index-run header.
pub(super) fn index_structure_digest(
    format_version: u32,
    header: &[u8; INDEX_HEADER_LEN],
    structure: &[u8],
) -> Result<[u8; 32]> {
    let mut hasher = index_structure_hasher(format_version, header)?;
    hasher.update(structure);
    Ok(hasher.finalize().into())
}

/// Authenticates streamed fence and filter bytes without joining them first.
pub(super) fn index_structure_digest_parts(
    format_version: u32,
    header: &[u8; INDEX_HEADER_LEN],
    fences: &[u8],
    filter: &[u8],
) -> Result<[u8; 32]> {
    let mut hasher = index_structure_hasher(format_version, header)?;
    hasher.update(fences);
    hasher.update(filter);
    Ok(hasher.finalize().into())
}

fn index_structure_hasher(format_version: u32, header: &[u8; INDEX_HEADER_LEN]) -> Result<Sha256> {
    if format_version != PACK_INDEX_FORMAT_VERSION {
        return Err(PackStoreError::unsupported_version(
            PackStoreArtifact::IndexRun,
            format_version,
            &[PACK_INDEX_FORMAT_VERSION],
        )
        .into());
    }
    let mut hasher = Sha256::new();
    hasher.update(INDEX_STRUCTURE_DIGEST_DOMAIN_V5);
    hasher.update(&header[..INDEX_STRUCTURE_SHA256_START]);
    hasher.update(&header[INDEX_STRUCTURE_SHA256_END..INDEX_HEADER_TAG_START]);
    Ok(hasher)
}
