//! Strictly verified, bounded, immutable NeoVM execution plans.

use super::ExecutionPlanKey;
use crate::{
    Instruction, MAX_SCRIPT_SIZE, OpCode, instruction_jump_target, instruction_try_targets,
    parse_script_instructions, validate_strict_script,
};
use std::collections::BTreeSet;
use std::fmt;

const NO_INSTRUCTION: u32 = u32::MAX;

/// Route selected while loading an explicitly plan-capable context.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExecutionPlanRoute {
    /// Exact identity matched and the immutable plan was attached.
    Planned,
    /// Ordinary `neo-vm` was loaded before any planned effect became visible.
    OrdinaryFallback,
}

/// Construction limits for an immutable execution plan.
///
/// Hitting any limit rejects plan construction and leaves ordinary lazy
/// `Script` execution available as the consensus fallback.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ExecutionPlanLimits {
    /// Maximum exact script bytes accepted by the plan builder.
    pub max_script_bytes: usize,
    /// Maximum decoded instructions retained by one plan.
    pub max_instructions: usize,
    /// Maximum conservative basic blocks retained by one plan.
    pub max_basic_blocks: usize,
    /// Maximum accounted immutable plan bytes.
    pub max_plan_bytes: usize,
}

impl ExecutionPlanLimits {
    /// Conservative production defaults. Legal scripts above these plan bounds
    /// continue through ordinary `neo-vm` without a plan.
    pub const DEFAULT: Self = Self {
        max_script_bytes: 256 * 1024,
        max_instructions: 256 * 1024,
        max_basic_blocks: 128 * 1024,
        max_plan_bytes: 16 * 1024 * 1024,
    };
}

impl Default for ExecutionPlanLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why immutable plan construction could not be completed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ExecutionPlanBuildError {
    /// The script exceeds the configured plan input bound.
    ScriptTooLarge {
        /// Observed byte length.
        actual: usize,
        /// Configured maximum byte length.
        maximum: usize,
    },
    /// The entry byte offset lies beyond the script end.
    EntryOutOfBounds {
        /// Requested entry byte offset.
        entry: u32,
        /// Exact script byte length.
        script_len: usize,
    },
    /// The entry is not the start of a decoded instruction.
    EntryNotInstruction {
        /// Requested entry byte offset.
        entry: u32,
    },
    /// Strict NeoVM parsing or validation rejected the script.
    InvalidScript(String),
    /// The decoded instruction count exceeds the configured bound.
    TooManyInstructions {
        /// Decoded instruction count.
        actual: usize,
        /// Configured maximum instruction count.
        maximum: usize,
    },
    /// The conservative block count exceeds the configured bound.
    TooManyBasicBlocks {
        /// Constructed basic-block count.
        actual: usize,
        /// Configured maximum basic-block count.
        maximum: usize,
    },
    /// Immutable plan storage exceeds the configured byte bound.
    PlanTooLarge {
        /// Accounted immutable plan bytes.
        actual: usize,
        /// Configured maximum plan bytes.
        maximum: usize,
    },
    /// A byte or instruction offset cannot be represented by the plan format.
    OffsetOverflow,
}

impl fmt::Display for ExecutionPlanBuildError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScriptTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "script has {actual} bytes; plan limit is {maximum}"
                )
            }
            Self::EntryOutOfBounds { entry, script_len } => {
                write!(
                    formatter,
                    "entry {entry} is beyond script length {script_len}"
                )
            }
            Self::EntryNotInstruction { entry } => {
                write!(formatter, "entry {entry} is not an instruction boundary")
            }
            Self::InvalidScript(message) => write!(formatter, "invalid script: {message}"),
            Self::TooManyInstructions { actual, maximum } => write!(
                formatter,
                "script has {actual} instructions; plan limit is {maximum}"
            ),
            Self::TooManyBasicBlocks { actual, maximum } => write!(
                formatter,
                "plan has {actual} basic blocks; plan limit is {maximum}"
            ),
            Self::PlanTooLarge { actual, maximum } => {
                write!(
                    formatter,
                    "plan accounts for {actual} bytes; limit is {maximum}"
                )
            }
            Self::OffsetOverflow => formatter.write_str("plan offset exceeds u32"),
        }
    }
}

impl std::error::Error for ExecutionPlanBuildError {}

/// Pre-resolved control-flow metadata for one consensus instruction.
///
/// The executor must still run the ordinary opcode handler. These values only
/// remove repeated operand decoding and identify conservative block exits.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlannedControlFlow {
    /// Execution normally continues at the next decoded instruction.
    Continue,
    /// An unconditional relative jump.
    Jump {
        /// Absolute target byte offset.
        target: u32,
    },
    /// A conditional relative jump with an explicit fallthrough.
    Branch {
        /// Absolute taken target byte offset.
        target: u32,
        /// Absolute not-taken byte offset.
        fallthrough: u32,
    },
    /// An intra-script call with an explicit return byte offset.
    Call {
        /// Absolute callee byte offset.
        target: u32,
        /// Absolute caller continuation byte offset.
        return_ip: u32,
    },
    /// An intra-script call through a runtime `Pointer` value.
    DynamicCall,
    /// A method-token call resolved by the application host.
    TokenCall {
        /// NEF method-token index.
        token: u16,
    },
    /// A try region with optional catch/finally targets.
    Try {
        /// Absolute catch entry, or `None` for a zero encoded offset.
        catch_target: Option<u32>,
        /// Absolute finally entry, or `None` for a zero encoded offset.
        finally_target: Option<u32>,
        /// Absolute first byte after the `TRY` instruction.
        fallthrough: u32,
    },
    /// End of a try region with a statically encoded target.
    EndTry {
        /// Absolute encoded end target.
        target: u32,
    },
    /// End of a finally region; the target is runtime exception state.
    EndFinally,
    /// Return from the current invocation context.
    Return,
    /// Host syscall with its pre-decoded service identifier.
    Syscall {
        /// Neo interop service identifier.
        service: u32,
    },
    /// Unconditional abort or throw.
    Fault,
    /// Assertion that either falls through or faults.
    Assert {
        /// Absolute continuation byte offset when the assertion succeeds.
        fallthrough: u32,
    },
}

impl PlannedControlFlow {
    const fn ends_basic_block(self) -> bool {
        !matches!(self, Self::Continue)
    }
}

/// One decoded immutable instruction and its plan metadata.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PlannedInstruction {
    instruction: Instruction,
    next_ip: u32,
    control_flow: PlannedControlFlow,
    address_target: Option<u32>,
}

impl PlannedInstruction {
    /// Returns the ordinary NeoVM instruction consumed by consensus handlers.
    #[must_use]
    pub const fn instruction(&self) -> &Instruction {
        &self.instruction
    }

    /// Returns the byte offset immediately after this instruction.
    #[must_use]
    pub const fn next_ip(&self) -> u32 {
        self.next_ip
    }

    /// Returns pre-resolved control-flow metadata.
    #[must_use]
    pub const fn control_flow(&self) -> PlannedControlFlow {
        self.control_flow
    }

    /// Returns the static same-script address produced by `PUSHA`, if any.
    #[must_use]
    pub const fn address_target(&self) -> Option<u32> {
        self.address_target
    }
}

/// A conservative sequence of instructions with one entry and no internal
/// planned control-flow boundary.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BasicBlock {
    start_ip: u32,
    end_ip: u32,
    first_instruction: u32,
    instruction_count: u32,
}

impl BasicBlock {
    /// First byte offset in the block.
    #[must_use]
    pub const fn start_ip(self) -> u32 {
        self.start_ip
    }

    /// Exclusive byte offset following the block.
    #[must_use]
    pub const fn end_ip(self) -> u32 {
        self.end_ip
    }

    /// Index of the first instruction in [`ExecutionPlan::instructions`].
    #[must_use]
    pub const fn first_instruction(self) -> u32 {
        self.first_instruction
    }

    /// Number of instructions in the block.
    #[must_use]
    pub const fn instruction_count(self) -> u32 {
        self.instruction_count
    }
}

/// Strictly verified immutable execution structure for one exact plan key.
///
/// This type contains no stack, gas, fault, storage, call, notification, or
/// other execution output. It is safe to discard or rebuild at any time.
#[derive(Clone, Debug)]
pub struct ExecutionPlan {
    key: ExecutionPlanKey,
    instructions: Box<[PlannedInstruction]>,
    instruction_by_offset: Box<[u32]>,
    basic_blocks: Box<[BasicBlock]>,
    accounted_bytes: usize,
}

impl ExecutionPlan {
    /// Builds a strict plan or rejects it before any execution effect exists.
    pub fn build(
        key: ExecutionPlanKey,
        limits: ExecutionPlanLimits,
    ) -> Result<Self, ExecutionPlanBuildError> {
        let script = key.script_bytes();
        if script.len() > MAX_SCRIPT_SIZE || script.len() > limits.max_script_bytes {
            return Err(ExecutionPlanBuildError::ScriptTooLarge {
                actual: script.len(),
                maximum: limits.max_script_bytes.min(MAX_SCRIPT_SIZE),
            });
        }
        let entry =
            usize::try_from(key.entry_ip()).map_err(|_| ExecutionPlanBuildError::OffsetOverflow)?;
        if entry > script.len() {
            return Err(ExecutionPlanBuildError::EntryOutOfBounds {
                entry: key.entry_ip(),
                script_len: script.len(),
            });
        }

        validate_strict_script(script).map_err(ExecutionPlanBuildError::InvalidScript)?;
        let decoded =
            parse_script_instructions(script).map_err(ExecutionPlanBuildError::InvalidScript)?;
        if decoded.len() > limits.max_instructions {
            return Err(ExecutionPlanBuildError::TooManyInstructions {
                actual: decoded.len(),
                maximum: limits.max_instructions,
            });
        }

        let mut instruction_by_offset = vec![NO_INSTRUCTION; script.len()].into_boxed_slice();
        let mut block_starts = BTreeSet::new();
        if !decoded.is_empty() {
            block_starts.insert(0usize);
        }
        let mut instructions = Vec::with_capacity(decoded.len());
        for (index, instruction) in decoded.into_iter().enumerate() {
            let pointer = instruction.pointer();
            let next = pointer
                .checked_add(instruction.size())
                .ok_or(ExecutionPlanBuildError::OffsetOverflow)?;
            let next_ip =
                u32::try_from(next).map_err(|_| ExecutionPlanBuildError::OffsetOverflow)?;
            instruction_by_offset[pointer] =
                u32::try_from(index).map_err(|_| ExecutionPlanBuildError::OffsetOverflow)?;
            let (control_flow, address_target) = planned_flow(&instruction, next_ip)?;
            collect_block_starts(
                control_flow,
                address_target,
                next,
                script.len(),
                &mut block_starts,
            );
            instructions.push(PlannedInstruction {
                instruction,
                next_ip,
                control_flow,
                address_target,
            });
        }

        if entry < script.len() && instruction_by_offset[entry] == NO_INSTRUCTION {
            return Err(ExecutionPlanBuildError::EntryNotInstruction {
                entry: key.entry_ip(),
            });
        }
        if entry < script.len() {
            block_starts.insert(entry);
        }

        let basic_blocks = build_basic_blocks(&instructions, &block_starts)?;
        if basic_blocks.len() > limits.max_basic_blocks {
            return Err(ExecutionPlanBuildError::TooManyBasicBlocks {
                actual: basic_blocks.len(),
                maximum: limits.max_basic_blocks,
            });
        }

        let accounted_bytes = account_plan_bytes(
            key.script_len(),
            &instructions,
            instruction_by_offset.len(),
            basic_blocks.len(),
        )?;
        if accounted_bytes > limits.max_plan_bytes {
            return Err(ExecutionPlanBuildError::PlanTooLarge {
                actual: accounted_bytes,
                maximum: limits.max_plan_bytes,
            });
        }

        Ok(Self {
            key,
            instructions: instructions.into_boxed_slice(),
            instruction_by_offset,
            basic_blocks: basic_blocks.into_boxed_slice(),
            accounted_bytes,
        })
    }

    /// Exact versioned identity used to construct the plan.
    #[must_use]
    pub const fn key(&self) -> &ExecutionPlanKey {
        &self.key
    }

    /// Strictly decoded instructions in bytecode order.
    #[must_use]
    pub const fn instructions(&self) -> &[PlannedInstruction] {
        &self.instructions
    }

    /// Conservative basic blocks in bytecode order.
    #[must_use]
    pub const fn basic_blocks(&self) -> &[BasicBlock] {
        &self.basic_blocks
    }

    /// Direct byte-offset lookup without hashing or instruction parsing.
    #[must_use]
    pub fn instruction_at(&self, byte_offset: usize) -> Option<&PlannedInstruction> {
        let index = *self.instruction_by_offset.get(byte_offset)?;
        (index != NO_INSTRUCTION).then(|| &self.instructions[index as usize])
    }

    /// Exact bytes used for cache accounting and admission.
    #[must_use]
    pub const fn accounted_bytes(&self) -> usize {
        self.accounted_bytes
    }

    /// Verifies exact bytes in addition to the protocol Hash160 lookup hint.
    #[must_use]
    pub fn matches_script(&self, script_hash: &[u8; 20], script_bytes: &[u8]) -> bool {
        self.key.matches_script(script_hash, script_bytes)
    }
}

fn to_u32(offset: usize) -> Result<u32, ExecutionPlanBuildError> {
    u32::try_from(offset).map_err(|_| ExecutionPlanBuildError::OffsetOverflow)
}

fn static_target(instruction: &Instruction) -> Result<u32, ExecutionPlanBuildError> {
    instruction_jump_target(instruction)
        .map_err(ExecutionPlanBuildError::InvalidScript)
        .and_then(to_u32)
}

fn planned_flow(
    instruction: &Instruction,
    next_ip: u32,
) -> Result<(PlannedControlFlow, Option<u32>), ExecutionPlanBuildError> {
    use OpCode::{
        ABORT, ABORTMSG, ASSERT, ASSERTMSG, CALL, CALL_L, CALLA, CALLT, ENDFINALLY, ENDTRY,
        ENDTRY_L, JMP, JMP_L, JMPEQ, JMPEQ_L, JMPGE, JMPGE_L, JMPGT, JMPGT_L, JMPIF, JMPIF_L,
        JMPIFNOT, JMPIFNOT_L, JMPLE, JMPLE_L, JMPLT, JMPLT_L, JMPNE, JMPNE_L, PUSHA, RET, SYSCALL,
        THROW, TRY, TRY_L,
    };

    let opcode = instruction.opcode();
    let flow = match opcode {
        JMP | JMP_L => PlannedControlFlow::Jump {
            target: static_target(instruction)?,
        },
        JMPIF | JMPIF_L | JMPIFNOT | JMPIFNOT_L | JMPEQ | JMPEQ_L | JMPNE | JMPNE_L | JMPGT
        | JMPGT_L | JMPGE | JMPGE_L | JMPLT | JMPLT_L | JMPLE | JMPLE_L => {
            PlannedControlFlow::Branch {
                target: static_target(instruction)?,
                fallthrough: next_ip,
            }
        }
        CALL | CALL_L => PlannedControlFlow::Call {
            target: static_target(instruction)?,
            return_ip: next_ip,
        },
        CALLA => PlannedControlFlow::DynamicCall,
        CALLT => PlannedControlFlow::TokenCall {
            token: instruction.token_u16(),
        },
        TRY | TRY_L => {
            let (catch, finally) = instruction_try_targets(instruction)
                .map_err(ExecutionPlanBuildError::InvalidScript)?;
            let (catch_is_zero, finally_is_zero) = if opcode == TRY {
                (instruction.token_i8() == 0, instruction.token_i8_1() == 0)
            } else {
                (instruction.token_i32() == 0, instruction.token_i32_1() == 0)
            };
            PlannedControlFlow::Try {
                catch_target: (!catch_is_zero).then(|| to_u32(catch)).transpose()?,
                finally_target: (!finally_is_zero).then(|| to_u32(finally)).transpose()?,
                fallthrough: next_ip,
            }
        }
        ENDTRY | ENDTRY_L => PlannedControlFlow::EndTry {
            target: static_target(instruction)?,
        },
        ENDFINALLY => PlannedControlFlow::EndFinally,
        RET => PlannedControlFlow::Return,
        SYSCALL => PlannedControlFlow::Syscall {
            service: instruction.token_u32(),
        },
        ABORT | ABORTMSG | THROW => PlannedControlFlow::Fault,
        ASSERT | ASSERTMSG => PlannedControlFlow::Assert {
            fallthrough: next_ip,
        },
        _ => PlannedControlFlow::Continue,
    };
    let address_target = (opcode == PUSHA)
        .then(|| static_target(instruction))
        .transpose()?;
    Ok((flow, address_target))
}

fn collect_block_starts(
    flow: PlannedControlFlow,
    address_target: Option<u32>,
    next: usize,
    script_len: usize,
    starts: &mut BTreeSet<usize>,
) {
    match flow {
        PlannedControlFlow::Jump { target }
        | PlannedControlFlow::EndTry { target }
        | PlannedControlFlow::Call { target, .. } => {
            insert_block_start(starts, target, script_len);
        }
        PlannedControlFlow::Branch {
            target,
            fallthrough,
        } => {
            insert_block_start(starts, target, script_len);
            insert_block_start(starts, fallthrough, script_len);
        }
        PlannedControlFlow::Try {
            catch_target,
            finally_target,
            fallthrough,
        } => {
            if let Some(target) = catch_target {
                insert_block_start(starts, target, script_len);
            }
            if let Some(target) = finally_target {
                insert_block_start(starts, target, script_len);
            }
            insert_block_start(starts, fallthrough, script_len);
        }
        PlannedControlFlow::Assert { fallthrough } => {
            insert_block_start(starts, fallthrough, script_len);
        }
        _ => {}
    }
    if flow.ends_basic_block() && next < script_len {
        starts.insert(next);
    }
    if let Some(target) = address_target {
        insert_block_start(starts, target, script_len);
    }
}

fn insert_block_start(starts: &mut BTreeSet<usize>, target: u32, script_len: usize) {
    let target = target as usize;
    if target < script_len {
        starts.insert(target);
    }
}

fn build_basic_blocks(
    instructions: &[PlannedInstruction],
    starts: &BTreeSet<usize>,
) -> Result<Vec<BasicBlock>, ExecutionPlanBuildError> {
    let mut blocks = Vec::new();
    let mut first = 0usize;
    while first < instructions.len() {
        let start_ip = instructions[first].instruction.pointer();
        let mut end = first + 1;
        while end < instructions.len()
            && !instructions[end - 1].control_flow.ends_basic_block()
            && !starts.contains(&instructions[end].instruction.pointer())
        {
            end += 1;
        }
        blocks.push(BasicBlock {
            start_ip: to_u32(start_ip)?,
            end_ip: instructions[end - 1].next_ip,
            first_instruction: to_u32(first)?,
            instruction_count: to_u32(end - first)?,
        });
        first = end;
    }
    Ok(blocks)
}

fn account_plan_bytes(
    script_bytes: usize,
    instructions: &[PlannedInstruction],
    offset_slots: usize,
    basic_blocks: usize,
) -> Result<usize, ExecutionPlanBuildError> {
    let operand_bytes = instructions.iter().try_fold(0usize, |total, planned| {
        total
            .checked_add(planned.instruction.operand().len())
            .ok_or(ExecutionPlanBuildError::OffsetOverflow)
    })?;
    script_bytes
        .checked_add(
            instructions
                .len()
                .checked_mul(std::mem::size_of::<PlannedInstruction>())
                .ok_or(ExecutionPlanBuildError::OffsetOverflow)?,
        )
        .and_then(|total| total.checked_add(operand_bytes))
        .and_then(|total| total.checked_add(offset_slots.checked_mul(std::mem::size_of::<u32>())?))
        .and_then(|total| {
            total.checked_add(basic_blocks.checked_mul(std::mem::size_of::<BasicBlock>())?)
        })
        .ok_or(ExecutionPlanBuildError::OffsetOverflow)
}

#[cfg(test)]
#[path = "../tests/execution_plan/plan.rs"]
mod tests;
