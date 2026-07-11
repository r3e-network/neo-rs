//! # Archive index
//!
//! ## Boundary
//!
//! The index is derived from authoritative archive frames. It accelerates
//! lookup and clean startup but never changes opaque record bytes.
//!
//! ## Contents
//!
//! - `model`: Checksummed fixed-width index records.
//! - `persistent`: MDBX publication, lookup, rollback, and verification.
//! - `scan`: Sequential suffix replay and explicit full scrubbing.

mod model;
mod persistent;
mod scan;

pub(super) use model::{
    FrameLocation, IndexState, PositionedEncodedFrame, RowLocation, ScannedFrame,
};
pub(super) use persistent::ArchiveIndex;
pub(super) use scan::{ScanMode, read_frame_index, scan_archive, validate_published_tail};
