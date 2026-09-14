// Copyright (C) 2015-2025 The Neo Project.
//
// inventory.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms, with or without
// modifications are permitted.

//! Re-export of the Inventory trait from neo-primitives.
//!
//! The canonical `Inventory` trait now lives in [`neo_primitives`] so that
//! both neo-core (implementations) and neo-p2p (networking) can depend on
//! it without a circular dependency.

pub use neo_primitives::Inventory;
pub use neo_primitives::InventoryType;
