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
//! - **Strict mode**: Validates all instructions and control-flow operands on load
//! - **Relaxed mode**: Decodes only instructions reached during execution
//!
//! ## Example
//!
//! ```rust,ignore
//! use neo_core::neo_vm::Script;
//! use crate::OpCode;
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
use crate::execution_plan::ExecutionPlan;
use crate::{Instruction, parse_script_instructions};
use crate::{instruction_jump_target, instruction_try_targets};
use neo_crypto::Crypto;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ptr;
use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::{Arc, OnceLock};

const INSTRUCTION_CACHE_SEGMENT_SHIFT: usize = 6;
const INSTRUCTION_CACHE_SEGMENT_SIZE: usize = 1 << INSTRUCTION_CACHE_SEGMENT_SHIFT;
const INSTRUCTION_CACHE_SEGMENT_MASK: usize = INSTRUCTION_CACHE_SEGMENT_SIZE - 1;

/// Instruction storage strategy.
///
/// - **Eager**: All instructions are pre-parsed at construction time and stored
///   in an immutable `HashMap`. Lookups require no locking.
/// - **Lazy**: Instructions are parsed on first access and cached in segmented
///   atomic slots (used for relaxed-mode scripts that skip up-front parsing).
#[derive(Debug, Clone)]
enum InstructionCache {
    /// Pre-populated, immutable cache — no lock on the read path.
    Eager(Arc<HashMap<usize, Arc<Instruction>>>),
    /// Lazily populated cache — lock-free pointer slots for concurrent reads.
    Lazy(Arc<LazyInstructionCache>),
}

/// Segmented, lock-free instruction cache used by relaxed scripts.
///
/// Segments are allocated only when an instruction in that byte range is first
/// reached. A slot owns one strong `Arc<Instruction>` reference once populated.
/// Readers acquire a second strong reference from the stable raw pointer; misses
/// race with compare-and-swap and discard the losing parse. The cache itself
/// keeps every successful instruction alive until the last cloned `Script` drops.
#[derive(Debug)]
struct LazyInstructionCache {
    script_len: usize,
    segments: Box<[OnceLock<Box<[AtomicPtr<Instruction>]>>]>,
}

impl LazyInstructionCache {
    fn new(script_len: usize) -> Self {
        let segment_count = script_len.div_ceil(INSTRUCTION_CACHE_SEGMENT_SIZE);
        let segments = std::iter::repeat_with(OnceLock::new)
            .take(segment_count)
            .collect::<Vec<_>>()
            .into_boxed_slice();
        Self {
            script_len,
            segments,
        }
    }

    fn segment(&self, segment_index: usize) -> &[AtomicPtr<Instruction>] {
        self.segments[segment_index].get_or_init(|| {
            let segment_start = segment_index * INSTRUCTION_CACHE_SEGMENT_SIZE;
            let segment_len = (self.script_len - segment_start).min(INSTRUCTION_CACHE_SEGMENT_SIZE);
            std::iter::repeat_with(|| AtomicPtr::new(ptr::null_mut()))
                .take(segment_len)
                .collect::<Vec<_>>()
                .into_boxed_slice()
        })
    }

    #[cfg(test)]
    fn initialized_segment_count(&self) -> usize {
        self.segments
            .iter()
            .filter(|segment| segment.get().is_some())
            .count()
    }

    #[allow(unsafe_code)]
    fn get(&self, script: &[u8], position: usize) -> VmResult<Arc<Instruction>> {
        let segment = self.segment(position >> INSTRUCTION_CACHE_SEGMENT_SHIFT);
        let slot = &segment[position & INSTRUCTION_CACHE_SEGMENT_MASK];
        let cached = slot.load(Ordering::Acquire);
        if !cached.is_null() {
            // SAFETY: a populated slot owns one strong Arc reference and the
            // cache remains alive for the duration of this method call.
            unsafe { Arc::increment_strong_count(cached) };
            // SAFETY: the increment above creates the returned Arc ownership.
            return Ok(unsafe { Arc::from_raw(cached) });
        }

        let instruction = Arc::new(Instruction::parse(script, position)?);
        // SAFETY: converting a cloned Arc to a raw pointer transfers exactly
        // one strong reference to the cache slot on a successful CAS.
        let owned = Arc::into_raw(Arc::clone(&instruction)).cast_mut();
        match slot.compare_exchange(ptr::null_mut(), owned, Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => Ok(instruction),
            Err(existing) => {
                // SAFETY: the CAS failed, so the cache did not take ownership
                // of `owned`; reclaim the extra strong reference here.
                unsafe { drop(Arc::from_raw(owned)) };
                // SAFETY: the winning cache pointer owns a strong Arc ref.
                unsafe { Arc::increment_strong_count(existing) };
                // SAFETY: the increment above creates the returned Arc.
                Ok(unsafe { Arc::from_raw(existing) })
            }
        }
    }
}

impl Drop for LazyInstructionCache {
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        for segment in &mut self.segments {
            if let Some(slots) = segment.get_mut() {
                for slot in slots.iter_mut() {
                    let pointer = slot.swap(ptr::null_mut(), Ordering::AcqRel);
                    if !pointer.is_null() {
                        // SAFETY: each non-null slot owns exactly one strong Arc ref.
                        unsafe { drop(Arc::from_raw(pointer)) };
                    }
                }
            }
        }
    }
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
/// Scripts created with relaxed / no-validation constructors use a segmented
/// atomic cache that allocates each 64-byte range only when reached.
///
/// The hash code and protocol Hash160 are computed eagerly at construction
/// time to avoid hashing immutable script bytes on hot access paths.
#[derive(Debug, Clone)]
pub struct Script {
    /// The script data
    script: Arc<[u8]>,

    /// Cached instructions — either eagerly populated or lazily populated in
    /// lock-free segmented slots.
    instructions: InstructionCache,

    /// Whether strict mode is enabled
    strict_mode: bool,

    /// Eagerly computed hash code (no lock needed for reads).
    hash_code: u64,

    /// Eagerly computed protocol script hash (RIPEMD-160 of SHA-256).
    script_hash: [u8; 20],

    /// Optional immutable plan selected before this script becomes visible.
    execution_plan: Option<Arc<ExecutionPlan>>,
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
        let script_hash = Crypto::hash160(&script);
        let script_len = script.len();
        let script = Arc::<[u8]>::from(script);
        let mut s = Self {
            script,
            instructions: InstructionCache::Lazy(Arc::new(LazyInstructionCache::new(script_len))),
            strict_mode: false, // Start with false to allow parsing
            hash_code,
            script_hash,
            execution_plan: None,
        };

        if strict_mode {
            // Parse all instructions eagerly and promote to lock-free cache.
            let map = s.parse_all_instructions()?;
            s.instructions = InstructionCache::Eager(Arc::new(map));
            s.strict_mode = true;
            s.validate_strict()?;
        }

        Ok(s)
    }

    /// Creates a new script with default settings (non-strict mode).
    /// This provides backward compatibility for code expecting `Script::new(script)`.
    pub fn from(script: Vec<u8>) -> VmResult<Self> {
        Ok(Self::new_relaxed(script))
    }

    /// Creates a new script without validation - backward compatibility with C# API
    /// This matches the C# Script(byte[] script) constructor exactly
    #[must_use]
    pub fn new_from_bytes(script: Vec<u8>) -> Self {
        Self::new_relaxed(script)
    }

    /// Creates a new script without validation.
    #[must_use]
    pub fn new_relaxed(script: Vec<u8>) -> Self {
        Self::new_relaxed_from_arc(Arc::<[u8]>::from(script))
    }

    /// Creates a relaxed script from an already-shared bytecode buffer.
    ///
    /// Prefer this on transaction import when callers can avoid an intermediate
    /// `Vec` allocation before wrapping bytes in `Arc`.
    #[must_use]
    pub fn new_relaxed_from_arc(script: Arc<[u8]>) -> Self {
        let hash_code = Self::compute_hash(script.as_ref());
        let script_hash = Crypto::hash160(script.as_ref());
        let script_len = script.len();
        Self {
            script,
            instructions: InstructionCache::Lazy(Arc::new(LazyInstructionCache::new(script_len))),
            strict_mode: false,
            hash_code,
            script_hash,
            execution_plan: None,
        }
    }

    /// Creates a relaxed script by copying `script` once into shared storage.
    #[must_use]
    pub fn new_relaxed_from_slice(script: &[u8]) -> Self {
        Self::new_relaxed_from_arc(Arc::<[u8]>::from(script))
    }

    /// Parses all instructions from the script into a `HashMap` keyed by byte
    /// offset. Used during construction to build the eager (lock-free) cache.
    fn parse_all_instructions(&self) -> VmResult<HashMap<usize, Arc<Instruction>>> {
        let mut instructions = HashMap::new();

        for instruction in
            parse_script_instructions(self.script.as_ref()).map_err(VmError::invalid_script_msg)?
        {
            instructions.insert(instruction.pointer(), Arc::new(instruction));
        }

        Ok(instructions)
    }

    /// Validates the script.
    pub fn validate(&self) -> VmResult<()> {
        parse_script_instructions(self.script.as_ref())
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

        crate::validate_script(self.script.as_ref(), true)
            .map(|_| ())
            .map_err(VmError::invalid_script_msg)
    }

    /// Gets the instruction at the specified position.
    ///
    /// # Performance
    ///
    /// For strict-mode scripts the instruction cache is an immutable `HashMap`,
    /// so this is a plain hash-lookup with **no locking**. Relaxed-mode hits
    /// use an atomic slot; only the first access to a 64-byte range initializes
    /// that range and only the first access to an offset races to publish it.
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

            // Relaxed-mode path: lock-free atomic slot lookup after the first
            // reached instruction, with a CAS only when decoding a new offset.
            InstructionCache::Lazy(cache) => cache.get(self.script.as_ref(), position),
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
        self.script.to_vec()
    }

    /// Returns the script as a byte slice.
    /// This matches the C# implementation's `ToArray()` behavior exactly.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.script.as_ref()
    }

    /// Returns the immutable script allocation shared by this value.
    ///
    /// Versioned execution-plan keys retain exact bytes for collision-proof
    /// identity. Sharing this existing allocation avoids copying a deployed
    /// script on every enabled cache lookup.
    #[must_use]
    pub fn shared_bytes(&self) -> Arc<[u8]> {
        Arc::clone(&self.script)
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

    /// Returns the cached protocol script hash (RIPEMD-160 of SHA-256).
    #[inline]
    #[must_use]
    pub const fn script_hash(&self) -> [u8; 20] {
        self.script_hash
    }

    /// Attaches an immutable plan after exact-byte verification.
    ///
    /// The script value is consumed so routing cannot change after it is shared
    /// through an execution context.
    pub fn with_execution_plan(mut self, plan: Arc<ExecutionPlan>) -> VmResult<Self> {
        if !plan.matches_script(&self.script_hash, self.script.as_ref()) {
            return Err(VmError::invalid_operation_msg(
                "Execution plan does not match exact script bytes",
            ));
        }
        self.execution_plan = Some(plan);
        Ok(self)
    }

    /// Returns the immutable plan selected for this script, when enabled.
    #[must_use]
    pub fn execution_plan(&self) -> Option<&Arc<ExecutionPlan>> {
        self.execution_plan.as_ref()
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
        self.script.as_ref()
    }
}

#[cfg(test)]
#[path = "../tests/types/script.rs"]
mod tests;
