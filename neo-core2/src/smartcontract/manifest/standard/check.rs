use crate::smartcontract::manifest;

/// Standard represents smart-contract standard.
pub struct Standard {
    /// Manifest describes mandatory methods and events.
    pub manifest: manifest::Manifest,
    /// Base contains base standard.
    pub base: Option<Box<Standard>>,
    /// Optional contains optional contract methods.
    /// If contract contains method with the same name and parameter count,
    /// it must have signature declared by this contract.
    pub optional: Vec<manifest::Method>,
}
