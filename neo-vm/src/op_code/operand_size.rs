//! Operand size attribute implementation.
//!
//! This module provides the `OperandSizeAttribute` functionality exactly matching C# Neo.VM.OperandSizeAttribute.

// Matches C# using directives exactly:
// using System;

/// namespace Neo.VM -> [AttributeUsage(AttributeTargets.Field, `AllowMultiple` = false)]
/// public class `OperandSizeAttribute` : Attribute
/// Indicates the operand length of an `OpCode`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperandSizeAttribute {
    /// When it is greater than 0, indicates the size of the operand.
    /// public int Size { get; set; }
    pub size: i32,

    /// When it is greater than 0, indicates the size prefix of the operand.
    /// public int `SizePrefix` { get; set; }
    pub size_prefix: i32,
}

impl OperandSizeAttribute {
    /// Creates a new `OperandSizeAttribute` with default values
    pub const fn new() -> Self {
        Self {
            size: 0,
            size_prefix: 0,
        }
    }

    /// Creates a new `OperandSizeAttribute` with a fixed size
    pub const fn with_size(size: i32) -> Self {
        Self {
            size,
            size_prefix: 0,
        }
    }

    /// Creates a new `OperandSizeAttribute` with a size prefix
    pub const fn with_size_prefix(size_prefix: i32) -> Self {
        Self {
            size: 0,
            size_prefix,
        }
    }

    /// Helper mirroring C# OperandSizeAttribute.Fixed
    pub fn fixed(size: i32) -> Self {
        Self::with_size(size)
    }

    /// Helper mirroring C# OperandSizeAttribute.SizePrefix
    pub fn prefix(size_prefix: i32) -> Self {
        Self::with_size_prefix(size_prefix)
    }
}

// Alias for compatibility
pub type OperandSize = OperandSizeAttribute;

impl Default for OperandSizeAttribute {
    fn default() -> Self {
        Self::new()
    }
}
