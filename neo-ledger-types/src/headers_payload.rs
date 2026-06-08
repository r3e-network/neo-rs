// Copyright (C) 2015-2025 The Neo Project.
//
// headers_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms, with or without
// modifications are permitted.

//! `HeadersPayload` — the response payload to `GetHeaders` P2P messages.
//!
//! A wire-format container of up to [`MAX_HEADERS_COUNT`] [`Header`]s
//! sent in response to a `GetBlocks` / `GetHeaders` request. The empty
//! case is rejected on deserialisation (a peer that has zero headers to
//! return should not reply with an empty payload).
//!
//! ## Layering
//!
//! Sits in **Layer 1 (protocol)**. Depends only on:
//! - `neo-primitives` (Layer 0) — for `UInt160` / `UInt256`.
//! - `neo-io` (Layer 0) — for `Serializable` + `impl_serializable!`.
//! - `serde` / `serde_json` — for the canonical JSON projection used by
//!   the RPC server.
//!
//! The Header data type is local to this crate, so the wire payload
//! lives here too. Stateful verification (consensus, native-contract
//! checks, DataCache lookup) is the job of `neo-core`'s
//! `HeaderVerifyExt` extension trait.

use crate::Header;
use neo_io::{impl_serializable, IoError};
use serde::{Deserialize, Serialize};

/// Indicates the maximum number of headers sent each time.
pub const MAX_HEADERS_COUNT: usize = 2000;

/// This message is sent to respond to GetHeaders messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadersPayload {
    /// The list of headers.
    pub headers: Vec<Header>,
}

impl HeadersPayload {
    /// Creates a new headers payload.
    pub fn create(headers: Vec<Header>) -> Self {
        Self { headers }
    }
}

impl_serializable! {
    struct HeadersPayload {
        headers: var_array<Header> { max: MAX_HEADERS_COUNT },
    }
    validate(self_ref) {
        if self_ref.headers.is_empty() {
            return Err(IoError::invalid_data("Empty headers list"));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_io::{MemoryReader, Serializable};

    #[test]
    fn empty_headers_list_rejected() {
        // An empty vec deserialises to an empty var-array, then the
        // `validate(self_ref)` arm rejects it with IoError::InvalidData.
        let mut buffer = Vec::new();
        // Encode `var_int(0)` (empty array length prefix) manually.
        buffer.push(0u8);
        let mut reader = MemoryReader::new(&buffer);
        let result = <HeadersPayload as Serializable>::deserialize(&mut reader);
        assert!(result.is_err(), "empty headers list must be rejected");
    }
}
