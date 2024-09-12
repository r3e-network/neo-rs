
use crate::network::payloads::{IVerifiable, InventoryType};

/// Represents a message that can be relayed on the NEO network.
pub trait IInventory: IVerifiable {
    /// The type of the inventory.
    fn inventory_type(&self) -> InventoryType;
}
