// Copyright (C) 2015-2025 The Neo Project.
//
// i_inventory.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::inventory_type::InventoryType;
use crate::IVerifiable;
use crate::UInt256;

/// Represents a message that can be relayed on the NEO network.
pub trait IInventory: IVerifiable {
    /// The type of the inventory.
    fn inventory_type(&self) -> InventoryType;

    /// Gets the hash of the inventory item.
    fn hash(&mut self) -> UInt256;
}
