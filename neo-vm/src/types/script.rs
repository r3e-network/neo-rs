//! Script - Neo VM bytecode representation.
//!
//! This module provides the `Script` type for representing and parsing
//! Neo Virtual Machine bytecode.
//!
//! ## Overview
//!
//! A `Script` wraps bytecode and provides:
//! - Instruction parsing and caching
//! - Bounds checking and validation
//! - Hash code caching for performance
//!
//! ## Strict vs Relaxed Mode
//!
//! - **Strict mode**: Validates all instructions on load (default)
//! - **Relaxed mode**: Allows lazy validation (useful for testing)
//!
//! ## Example
//!
//! ```rust,ignore
//! use neo_core::neo_vm::Script;
//! use neo_vm_rs::OpCode;
//!
//! // Create a script from bytecode
//! let bytecode = vec![OpCode::PUSH1.byte(), OpCode::RET.byte()];
//! let script = Script::new(bytecode, false)?;
//!
//! // Iterate over instructions
//! for result in script.iter() {
//!     let (position, instruction): (usize, _) = result?;
//!     println!("{}: {:?}", position, instruction.opcode());
//! }
//! ```

use crate::error::VmError;
use crate::error::VmResult;
use neo_vm_rs::{Instruction, parse_script_instructions};
use neo_vm_rs::{instruction_jump_target, instruction_try_targets};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ptr;
use std::sync::Arc;

/// Instruction storage strategy.
///
/// - **Eager**: All instructions are pre-parsed at construction time and stored
///   in an immutable `HashMap`. Lookups require no locking.
/// - **Lazy**: Instructions are parsed on first access and cached behind a
///   `RwLock` (only used for relaxed-mode scripts that skip up-front parsing).
#[derive(Debug, Clone)]
enum InstructionCache {
    /// Pre-populated, immutable cache — no lock on the read path.
    Eager(HashMap<usize, Arc<Instruction>>),
    /// Lazily populated cache — `RwLock` for concurrent reads, rare writes.
    Lazy(Arc<RwLock<HashMap<usize, Arc<Instruction>>>>),
}

/// Represents a script in the Neo VM.
///
/// # Performance
///
/// When constructed with strict mode (or via [`Script::new`] with
/// `strict_mode = true`), all instructions are parsed eagerly and stored in a
/// plain `HashMap`. The hot `get_instruction()` path then performs a single
/// `HashMap::get` with **no locking**.
///
/// Scripts created with relaxed / no-validation constructors use a
/// `RwLock<HashMap>` that lazily caches instructions on first access.
///
/// The hash code is computed eagerly at construction time to avoid any
/// synchronisation overhead on the `hash()` / `hash_code()` accessors.
#[derive(Debug, Clone)]
pub struct Script {
    /// The script data
    script: Vec<u8>,

    /// Cached instructions — either eagerly populated (lock-free) or lazily
    /// populated behind a `RwLock`.
    instructions: InstructionCache,

    /// Whether strict mode is enabled
    strict_mode: bool,

    /// Eagerly computed hash code (no lock needed for reads).
    hash_code: u64,
}

impl PartialEq for Script {
    fn eq(&self, other: &Self) -> bool {
        ptr::eq(self, other)
    }
}

impl Eq for Script {}

impl Hash for Script {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self as *const Self).hash(state);
    }
}

/// Iterator over the instructions in a script.
pub struct InstructionIterator<'a> {
    script: &'a Script,
    position: usize,
}

impl Iterator for InstructionIterator<'_> {
    type Item = VmResult<(usize, Arc<Instruction>)>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.position >= self.script.len() {
            return None;
        }

        match self.script.get_instruction(self.position) {
            Ok(instruction) => {
                let current_position = self.position;
                self.position += instruction.size();
                Some(Ok((current_position, instruction)))
            }
            Err(error) => Some(Err(error)),
        }
    }
}

impl Script {
    /// Computes the hash code for the given script bytes.
    fn compute_hash(script: &[u8]) -> u64 {
        let mut hasher = DefaultHasher::new();
        script.hash(&mut hasher);
        hasher.finish()
    }

    /// Creates a new script with optional validation and strict mode.
    pub fn new(script: Vec<u8>, strict_mode: bool) -> VmResult<Self> {
        let hash_code = Self::compute_hash(&script);
        let mut s = Self {
            script,
            instructions: InstructionCache::Lazy(Arc::new(RwLock::new(HashMap::new()))),
            strict_mode: false, // Start with false to allow parsing
            hash_code,
        };

        if strict_mode {
            // Parse all instructions eagerly and promote to lock-free cache.
            let map = s.parse_all_instructions()?;
            s.instructions = InstructionCache::Eager(map);
            s.strict_mode = true;
            s.validate_strict()?;
        } else {
            s.validate()?;
        }

        Ok(s)
    }

    /// Creates a new script with default settings (non-strict mode).
    /// This provides backward compatibility for code expecting `Script::new(script)`.
    pub fn from(script: Vec<u8>) -> VmResult<Self> {
        Self::new(script, false)
    }

    /// Creates a new script without validation - backward compatibility with C# API
    /// This matches the C# Script(byte[] script) constructor exactly
    #[must_use]
    pub fn new_from_bytes(script: Vec<u8>) -> Self {
        let hash_code = Self::compute_hash(&script);
        Self {
            script,
            instructions: InstructionCache::Lazy(Arc::new(RwLock::new(HashMap::new()))),
            strict_mode: false,
            hash_code,
        }
    }

    /// Creates a new script without validation.
    #[must_use]
    pub fn new_relaxed(script: Vec<u8>) -> Self {
        let hash_code = Self::compute_hash(&script);
        Self {
            script,
            instructions: InstructionCache::Lazy(Arc::new(RwLock::new(HashMap::new()))),
            strict_mode: false,
            hash_code,
        }
    }

    /// Parses all instructions from the script into a `HashMap` keyed by byte
    /// offset. Used during construction to build the eager (lock-free) cache.
    fn parse_all_instructions(&self) -> VmResult<HashMap<usize, Arc<Instruction>>> {
        let mut instructions = HashMap::new();

        for instruction in
            parse_script_instructions(&self.script).map_err(VmError::invalid_script_msg)?
        {
            instructions.insert(instruction.pointer(), Arc::new(instruction));
        }

        Ok(instructions)
    }

    /// Validates the script.
    pub fn validate(&self) -> VmResult<()> {
        parse_script_instructions(&self.script)
            .map(|_| ())
            .map_err(VmError::invalid_script_msg)
    }

    /// Validates the script in strict mode.
    pub fn validate_strict(&self) -> VmResult<()> {
        match &self.instructions {
            InstructionCache::Eager(_) => {}
            InstructionCache::Lazy(_) => {
                return Err(VmError::invalid_operation_msg(
                    "validate_strict requires an eagerly populated instruction cache",
                ));
            }
        }

        neo_vm_rs::validate_script(&self.script, true)
            .map(|_| ())
            .map_err(VmError::invalid_script_msg)
    }

    /// Gets the instruction at the specified position.
    ///
    /// # Performance
    ///
    /// For strict-mode scripts the instruction cache is an immutable `HashMap`,
    /// so this is a plain hash-lookup with **no locking**. For relaxed-mode
    /// scripts a `RwLock`-guarded cache is used (read lock on hit, write lock
    /// on miss).
    pub fn get_instruction(&self, position: usize) -> VmResult<Arc<Instruction>> {
        if position >= self.script.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Position {position} is beyond script bounds"
            )));
        }

        match &self.instructions {
            // Fast path: lock-free lookup in the pre-populated cache.
            // Arc::clone is just an atomic increment — no data copying.
            InstructionCache::Eager(map) => match map.get(&position) {
                Some(instruction) => Ok(Arc::clone(instruction)),
                None => Err(VmError::invalid_operation_msg(format!(
                    "Position {position} not found with strict mode"
                ))),
            },

            // Relaxed-mode path: read lock for cache hit, write lock for miss.
            InstructionCache::Lazy(cache) => {
                // Try read lock first (common case after first access).
                {
                    let instructions = cache.read();
                    if let Some(instruction) = instructions.get(&position) {
                        return Ok(Arc::clone(instruction));
                    }
                }

                // Cache miss - parse, wrap in Arc, and insert under write lock.
                let instruction = Arc::new(Instruction::parse(&self.script, position)?);

                {
                    let mut instructions = cache.write();
                    instructions.insert(position, Arc::clone(&instruction));
                }

                Ok(instruction)
            }
        }
    }

    /// Gets a byte at the specified position.
    pub fn get_byte(&self, position: usize) -> VmResult<u8> {
        if position >= self.script.len() {
            return Err(VmError::invalid_operation_msg(format!(
                "Position {position} is beyond script bounds"
            )));
        }

        Ok(self.script[position])
    }

    /// Gets a range of bytes from the script.
    pub fn range(&self, start: usize, end: usize) -> VmResult<Vec<u8>> {
        if start >= self.script.len() || end > self.script.len() || start > end {
            return Err(VmError::invalid_operation_msg(format!(
                "Range {start}..{end} is invalid"
            )));
        }

        Ok(self.script[start..end].to_vec())
    }

    /// Returns the script as a byte array.
    #[must_use]
    pub fn to_array(&self) -> Vec<u8> {
        self.script.clone()
    }

    /// Returns the script as a byte slice.
    /// This matches the C# implementation's `ToArray()` behavior exactly.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.script
    }

    /// Returns the length of the script.
    #[must_use]
    pub fn len(&self) -> usize {
        self.script.len()
    }

    /// Returns the length of the script - C# API compatibility
    /// This matches the C# Script.Length property exactly
    #[must_use]
    pub fn length(&self) -> usize {
        self.script.len()
    }

    /// Returns true if the script is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.script.is_empty()
    }

    /// Returns an iterator over the instructions in the script.
    ///
    /// # Returns
    ///
    /// An iterator over the instructions in the script
    #[must_use]
    pub const fn instructions(&self) -> InstructionIterator<'_> {
        InstructionIterator {
            script: self,
            position: 0,
        }
    }

    /// Calculates the offset for a jump instruction.
    ///
    /// # Arguments
    ///
    /// * `next_position` - The position after the jump instruction (where offset is relative to)
    /// * `offset` - The jump offset from the next instruction
    ///
    /// # Returns
    ///
    /// The absolute position after the jump
    pub fn get_jump_offset(&self, next_position: usize, offset: i32) -> VmResult<usize> {
        let new_position = next_position as i32 + offset;

        if new_position < 0 || new_position >= self.script.len() as i32 {
            return Err(VmError::invalid_script_msg("Jump offset out of bounds"));
        }

        Ok(new_position as usize)
    }

    /// Calculates the hash of the script.
    ///
    /// # Returns
    ///
    /// The hash of the script as a byte array
    #[must_use]
    pub fn hash(&self) -> Vec<u8> {
        self.hash_code.to_le_bytes().to_vec()
    }

    /// Gets the hash code of the script.
    #[must_use]
    pub fn hash_code(&self) -> u64 {
        self.hash_code
    }

    /// Calculates the jump target for a jump instruction.
    ///
    /// # Arguments
    ///
    /// * `instruction` - The jump instruction
    ///
    /// # Returns
    ///
    /// The absolute position of the jump target
    pub fn get_jump_target(&self, instruction: &Instruction) -> VmResult<usize> {
        let target =
            instruction_jump_target(instruction).map_err(VmError::invalid_instruction_msg)?;
        if target >= self.script.len() {
            return Err(VmError::invalid_script_msg("Jump offset out of bounds"));
        }
        Ok(target)
    }

    /// Calculates the try-catch-finally offsets for a TRY instruction.
    ///
    /// # Arguments
    ///
    /// * `instruction` - The TRY instruction
    ///
    /// # Returns
    ///
    /// A tuple of (`catch_offset`, `finally_offset`) as absolute positions
    pub fn get_try_offsets(&self, instruction: &Instruction) -> VmResult<(usize, usize)> {
        let (catch_position, finally_position) =
            instruction_try_targets(instruction).map_err(VmError::invalid_instruction_msg)?;
        if catch_position >= self.script.len() || finally_position >= self.script.len() {
            return Err(VmError::invalid_script_msg("Jump offset out of bounds"));
        }
        Ok((catch_position, finally_position))
    }

    /// Gets the next instruction position after the given position.
    ///
    /// # Arguments
    ///
    /// * `position` - The current instruction position
    ///
    /// # Returns
    ///
    /// A tuple of (instruction, `next_position`)
    pub fn get_next_instruction(&self, position: usize) -> VmResult<(Arc<Instruction>, usize)> {
        let instruction = self.get_instruction(position)?;
        let next_position = position + instruction.size();
        Ok((instruction, next_position))
    }
}

impl AsRef<[u8]> for Script {
    fn as_ref(&self) -> &[u8] {
        &self.script
    }
}

#[cfg(test)]
#[path = "../tests/types/script.rs"]
mod tests;
