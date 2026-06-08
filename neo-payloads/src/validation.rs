// Copyright (C) 2015-2025 The Neo Project.
//
// validation.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Block validation constants re-exported for the payload layer.
//!
//! The full block-validation helpers (block-size, transaction-count, primary
//! index, timestamp progression, merkle checks) live in `neo-core` because
//! they need access to `DataCache` plus the native contracts. The constants
//! below are pure values that the payload layer needs to do structural
//! validation.

/// Minimum valid timestamp (Neo genesis block timestamp: July 15, 2016)
pub const MIN_TIMESTAMP_MS: u64 = 1468595301000;

/// Maximum allowed timestamp drift from current time (15 minutes in milliseconds)
pub const MAX_TIMESTAMP_DRIFT_MS: u64 = 15 * 60 * 1000;
