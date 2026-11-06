// Copyright (C) 2015-2025 The Neo Project.
//
// headers_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Headers payload re-exported from `neo-p2p` (task 199 consolidation).
//!
//! The canonical `HeadersPayload` (built on `neo_protocol::payloads::Header`,
//! the same type re-exported by `super::header`) lives in
//! `neo_p2p::payloads::headers_payload`.

pub use neo_p2p::payloads::headers_payload::{HeadersPayload, MAX_HEADERS_COUNT};
