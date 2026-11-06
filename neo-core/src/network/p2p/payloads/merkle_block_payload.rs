// Copyright (C) 2015-2025 The Neo Project.
//
// merkle_block_payload.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Merkle block payload re-exported from `neo-p2p` (task 199 consolidation).
//!
//! The canonical `MerkleBlockPayload` (built on `neo_protocol` `Block`/`Header`,
//! the same types re-exported by `super::block` / `super::header`) lives in
//! `neo_p2p::payloads::merkle_block_payload`. Note the canonical constructors
//! take `&Block` (not `&mut Block`) and `try_create` returns `IoResult`.

pub use neo_p2p::payloads::merkle_block_payload::MerkleBlockPayload;
