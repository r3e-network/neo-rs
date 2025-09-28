//! IHardforkActivable - matches C# Neo.SmartContract.Native.IHardforkActivable exactly

/// Interface for hardfork-activable features (matches C# IHardforkActivable)
pub trait IHardforkActivable {
    /// Called when a hardfork is activated
    fn on_hardfork(&mut self, hardfork: &str, block_index: u32) -> Result<(), String>;
    
    /// Checks if a hardfork is active
    fn is_hardfork_active(&self, hardfork: &str, block_index: u32) -> bool;
    
    /// Gets the activation height for a hardfork
    fn get_hardfork_activation_height(&self, hardfork: &str) -> Option<u32>;
}