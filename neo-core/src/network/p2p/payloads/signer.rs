// Copyright (C) 2015-2025 The Neo Project.
//
// signer.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Transaction signer.
//!
//! The canonical `Signer` payload is owned by `neo-protocol` and re-exported
//! here for backward compatibility (R2 leaf-type unification). All structural
//! API (`new`, `scopes`, `get_all_rules`, `to_json`, `from_json`, wire
//! `Serializable`, and `Hash`) is provided by `neo-protocol`.
//!
//! neo-core keeps only the VM projection helpers, which depend on `neo-vm`
//! stack types that the protocol layer does not reference.

/// Maximum number of allowed contracts, groups, or witness rules on a signer.
pub use neo_protocol::payloads::signer::MAX_SIGNER_SUBITEMS;
/// Canonical transaction signer payload, owned by `neo-protocol`.
pub use neo_protocol::payloads::signer::Signer;

mod stack_projection;
pub use stack_projection::{signer_to_stack_item, signer_to_stack_value};
