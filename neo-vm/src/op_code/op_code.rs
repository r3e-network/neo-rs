//! Implementation of the `OpCode` enum for the Neo Virtual Machine.
//!
//! This module defines all opcodes supported by the Neo N3 Virtual Machine,
//! matching the C# Neo implementation exactly.

use super::operand_size::OperandSize;

const HASH_SIZE: usize = 32;

/// Represents the opcode of an instruction in the Neo Virtual Machine.
///
/// Each opcode corresponds to a specific operation that the VM can execute.
/// The opcodes are organized into categories: constants, flow control, stack,
/// slot, splice, bitwise, numeric, compound types, and type operations.
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum OpCode {
    // ==================== Constants ====================
    /// Push a signed 8-bit integer onto the stack.
    PUSHINT8 = 0x00,
    /// Push a signed 16-bit integer onto the stack.
    PUSHINT16 = 0x01,
    /// Push a signed 32-bit integer onto the stack.
    PUSHINT32 = 0x02,
    /// Push a signed 64-bit integer onto the stack.
    PUSHINT64 = 0x03,
    /// Push a signed 128-bit integer onto the stack.
    PUSHINT128 = 0x04,
    /// Push a signed 256-bit integer onto the stack.
    PUSHINT256 = 0x05,
    /// Push the boolean value `true` onto the stack.
    PUSHT = 0x08,
    /// Push the boolean value `false` onto the stack.
    PUSHF = 0x09,
    /// Push a pointer (script offset) onto the stack.
    PUSHA = 0x0A,
    /// Push a null reference onto the stack.
    PUSHNULL = 0x0B,
    /// Push data with 1-byte length prefix onto the stack.
    PUSHDATA1 = 0x0C,
    /// Push data with 2-byte length prefix onto the stack.
    PUSHDATA2 = 0x0D,
    /// Push data with 4-byte length prefix onto the stack.
    PUSHDATA4 = 0x0E,
    /// Push the integer -1 onto the stack.
    PUSHM1 = 0x0F,
    /// Push the integer 0 onto the stack.
    PUSH0 = 0x10,
    /// Push the integer 1 onto the stack.
    PUSH1 = 0x11,
    /// Push the integer 2 onto the stack.
    PUSH2 = 0x12,
    /// Push the integer 3 onto the stack.
    PUSH3 = 0x13,
    /// Push the integer 4 onto the stack.
    PUSH4 = 0x14,
    /// Push the integer 5 onto the stack.
    PUSH5 = 0x15,
    /// Push the integer 6 onto the stack.
    PUSH6 = 0x16,
    /// Push the integer 7 onto the stack.
    PUSH7 = 0x17,
    /// Push the integer 8 onto the stack.
    PUSH8 = 0x18,
    /// Push the integer 9 onto the stack.
    PUSH9 = 0x19,
    /// Push the integer 10 onto the stack.
    PUSH10 = 0x1A,
    /// Push the integer 11 onto the stack.
    PUSH11 = 0x1B,
    /// Push the integer 12 onto the stack.
    PUSH12 = 0x1C,
    /// Push the integer 13 onto the stack.
    PUSH13 = 0x1D,
    /// Push the integer 14 onto the stack.
    PUSH14 = 0x1E,
    /// Push the integer 15 onto the stack.
    PUSH15 = 0x1F,
    /// Push the integer 16 onto the stack.
    PUSH16 = 0x20,

    // ==================== Flow Control ====================
    /// No operation. Does nothing.
    NOP = 0x21,
    /// Unconditional jump with 1-byte offset.
    JMP = 0x22,
    /// Unconditional jump with 4-byte offset.
    JMP_L = 0x23,
    /// Jump if top of stack is true (1-byte offset).
    JMPIF = 0x24,
    /// Jump if top of stack is true (4-byte offset).
    JMPIF_L = 0x25,
    /// Jump if top of stack is false (1-byte offset).
    JMPIFNOT = 0x26,
    /// Jump if top of stack is false (4-byte offset).
    JMPIFNOT_L = 0x27,
    /// Jump if two values are equal (1-byte offset).
    JMPEQ = 0x28,
    /// Jump if two values are equal (4-byte offset).
    JMPEQ_L = 0x29,
    /// Jump if two values are not equal (1-byte offset).
    JMPNE = 0x2A,
    /// Jump if two values are not equal (4-byte offset).
    JMPNE_L = 0x2B,
    /// Jump if first value is greater than second (1-byte offset).
    JMPGT = 0x2C,
    /// Jump if first value is greater than second (4-byte offset).
    JMPGT_L = 0x2D,
    /// Jump if first value is greater than or equal to second (1-byte offset).
    JMPGE = 0x2E,
    /// Jump if first value is greater than or equal to second (4-byte offset).
    JMPGE_L = 0x2F,
    /// Jump if first value is less than second (1-byte offset).
    JMPLT = 0x30,
    /// Jump if first value is less than second (4-byte offset).
    JMPLT_L = 0x31,
    /// Jump if first value is less than or equal to second (1-byte offset).
    JMPLE = 0x32,
    /// Jump if first value is less than or equal to second (4-byte offset).
    JMPLE_L = 0x33,
    /// Call a function with 1-byte offset.
    CALL = 0x34,
    /// Call a function with 4-byte offset.
    CALL_L = 0x35,
    /// Call a function pointer from the stack.
    CALLA = 0x36,
    /// Call a token (contract method).
    CALLT = 0x37,
    /// Abort execution unconditionally.
    ABORT = 0x38,
    /// Assert that top of stack is true, abort if false.
    ASSERT = 0x39,
    /// Throw an exception.
    THROW = 0x3A,
    /// Begin a try block with 1-byte offsets.
    TRY = 0x3B,
    /// Begin a try block with 4-byte offsets.
    TRY_L = 0x3C,
    /// End a try block with 1-byte offset.
    ENDTRY = 0x3D,
    /// End a try block with 4-byte offset.
    ENDTRY_L = 0x3E,
    /// End a finally block.
    ENDFINALLY = 0x3F,
    /// Return from the current function.
    RET = 0x40,
    /// Make a system call.
    SYSCALL = 0x41,

    // ==================== Stack Operations ====================
    /// Get the number of items on the stack.
    DEPTH = 0x43,
    /// Remove the top item from the stack.
    DROP = 0x45,
    /// Remove the second item from the stack.
    NIP = 0x46,
    /// Remove the item at index n from the stack.
    XDROP = 0x48,
    /// Clear all items from the stack.
    CLEAR = 0x49,
    /// Duplicate the top item on the stack.
    DUP = 0x4A,
    /// Copy the second item to the top of the stack.
    OVER = 0x4B,
    /// Copy the item at index n to the top of the stack.
    PICK = 0x4D,
    /// Copy the top item and insert it before the second item.
    TUCK = 0x4E,
    /// Swap the top two items on the stack.
    SWAP = 0x50,
    /// Rotate the top three items on the stack.
    ROT = 0x51,
    /// Move the item at index n to the top of the stack.
    ROLL = 0x52,
    /// Reverse the order of the top 3 items on the stack.
    REVERSE3 = 0x53,
    /// Reverse the order of the top 4 items on the stack.
    REVERSE4 = 0x54,
    /// Reverse the order of the top n items on the stack.
    REVERSEN = 0x55,

    // ==================== Slot Operations ====================
    /// Initialize static field slots.
    INITSSLOT = 0x56,
    /// Initialize local variable and argument slots.
    INITSLOT = 0x57,
    /// Load static field 0 onto the stack.
    LDSFLD0 = 0x58,
    /// Load static field 1 onto the stack.
    LDSFLD1 = 0x59,
    /// Load static field 2 onto the stack.
    LDSFLD2 = 0x5A,
    /// Load static field 3 onto the stack.
    LDSFLD3 = 0x5B,
    /// Load static field 4 onto the stack.
    LDSFLD4 = 0x5C,
    /// Load static field 5 onto the stack.
    LDSFLD5 = 0x5D,
    /// Load static field 6 onto the stack.
    LDSFLD6 = 0x5E,
    /// Load static field at index onto the stack.
    LDSFLD = 0x5F,
    /// Store value into static field 0.
    STSFLD0 = 0x60,
    /// Store value into static field 1.
    STSFLD1 = 0x61,
    /// Store value into static field 2.
    STSFLD2 = 0x62,
    /// Store value into static field 3.
    STSFLD3 = 0x63,
    /// Store value into static field 4.
    STSFLD4 = 0x64,
    /// Store value into static field 5.
    STSFLD5 = 0x65,
    /// Store value into static field 6.
    STSFLD6 = 0x66,
    /// Store value into static field at index.
    STSFLD = 0x67,
    /// Load local variable 0 onto the stack.
    LDLOC0 = 0x68,
    /// Load local variable 1 onto the stack.
    LDLOC1 = 0x69,
    /// Load local variable 2 onto the stack.
    LDLOC2 = 0x6A,
    /// Load local variable 3 onto the stack.
    LDLOC3 = 0x6B,
    /// Load local variable 4 onto the stack.
    LDLOC4 = 0x6C,
    /// Load local variable 5 onto the stack.
    LDLOC5 = 0x6D,
    /// Load local variable 6 onto the stack.
    LDLOC6 = 0x6E,
    /// Load local variable at index onto the stack.
    LDLOC = 0x6F,
    /// Store value into local variable 0.
    STLOC0 = 0x70,
    /// Store value into local variable 1.
    STLOC1 = 0x71,
    /// Store value into local variable 2.
    STLOC2 = 0x72,
    /// Store value into local variable 3.
    STLOC3 = 0x73,
    /// Store value into local variable 4.
    STLOC4 = 0x74,
    /// Store value into local variable 5.
    STLOC5 = 0x75,
    /// Store value into local variable 6.
    STLOC6 = 0x76,
    /// Store value into local variable at index.
    STLOC = 0x77,
    /// Load argument 0 onto the stack.
    LDARG0 = 0x78,
    /// Load argument 1 onto the stack.
    LDARG1 = 0x79,
    /// Load argument 2 onto the stack.
    LDARG2 = 0x7A,
    /// Load argument 3 onto the stack.
    LDARG3 = 0x7B,
    /// Load argument 4 onto the stack.
    LDARG4 = 0x7C,
    /// Load argument 5 onto the stack.
    LDARG5 = 0x7D,
    /// Load argument 6 onto the stack.
    LDARG6 = 0x7E,
    /// Load argument at index onto the stack.
    LDARG = 0x7F,
    /// Store value into argument 0.
    STARG0 = 0x80,
    /// Store value into argument 1.
    STARG1 = 0x81,
    /// Store value into argument 2.
    STARG2 = 0x82,
    /// Store value into argument 3.
    STARG3 = 0x83,
    /// Store value into argument 4.
    STARG4 = 0x84,
    /// Store value into argument 5.
    STARG5 = 0x85,
    /// Store value into argument 6.
    STARG6 = 0x86,
    /// Store value into argument at index.
    STARG = 0x87,

    // ==================== Splice Operations ====================
    /// Create a new buffer of specified size.
    NEWBUFFER = 0x88,
    /// Copy memory from one buffer to another.
    MEMCPY = 0x89,
    /// Concatenate two byte arrays.
    CAT = 0x8B,
    /// Extract a substring from a byte array.
    SUBSTR = 0x8C,
    /// Extract the left part of a byte array.
    LEFT = 0x8D,
    /// Extract the right part of a byte array.
    RIGHT = 0x8E,

    // ==================== Bitwise Operations ====================
    /// Bitwise NOT (invert all bits).
    INVERT = 0x90,
    /// Bitwise AND of two integers.
    AND = 0x91,
    /// Bitwise OR of two integers.
    OR = 0x92,
    /// Bitwise XOR of two integers.
    XOR = 0x93,
    /// Check if two items are equal.
    EQUAL = 0x97,
    /// Check if two items are not equal.
    NOTEQUAL = 0x98,

    // ==================== Numeric Operations ====================
    /// Get the sign of an integer (-1, 0, or 1).
    SIGN = 0x99,
    /// Get the absolute value of an integer.
    ABS = 0x9A,
    /// Negate an integer.
    NEGATE = 0x9B,
    /// Increment an integer by 1.
    INC = 0x9C,
    /// Decrement an integer by 1.
    DEC = 0x9D,
    /// Add two integers.
    ADD = 0x9E,
    /// Subtract two integers.
    SUB = 0x9F,
    /// Multiply two integers.
    MUL = 0xA0,
    /// Divide two integers.
    DIV = 0xA1,
    /// Get the remainder of integer division.
    MOD = 0xA2,
    /// Raise an integer to a power.
    POW = 0xA3,
    /// Get the square root of an integer.
    SQRT = 0xA4,
    /// Modular multiplication: (a * b) % modulus.
    MODMUL = 0xA5,
    /// Modular exponentiation: (base ^ exp) % modulus.
    MODPOW = 0xA6,
    /// Shift left by n bits.
    SHL = 0xA8,
    /// Shift right by n bits.
    SHR = 0xA9,
    /// Logical NOT (boolean negation).
    NOT = 0xAA,
    /// Logical AND of two booleans.
    BOOLAND = 0xAB,
    /// Logical OR of two booleans.
    BOOLOR = 0xAC,
    /// Check if value is non-zero.
    NZ = 0xB1,
    /// Check if two integers are equal.
    NUMEQUAL = 0xB3,
    /// Check if two integers are not equal.
    NUMNOTEQUAL = 0xB4,
    /// Check if first integer is less than second.
    LT = 0xB5,
    /// Check if first integer is less than or equal to second.
    LE = 0xB6,
    /// Check if first integer is greater than second.
    GT = 0xB7,
    /// Check if first integer is greater than or equal to second.
    GE = 0xB8,
    /// Get the minimum of two integers.
    MIN = 0xB9,
    /// Get the maximum of two integers.
    MAX = 0xBA,
    /// Check if value is within range [min, max).
    WITHIN = 0xBB,

    // ==================== Compound Type Operations ====================
    /// Pack key-value pairs into a map.
    PACKMAP = 0xBE,
    /// Pack items into a struct.
    PACKSTRUCT = 0xBF,
    /// Pack items into an array.
    PACK = 0xC0,
    /// Unpack an array onto the stack.
    UNPACK = 0xC1,
    /// Create an empty array.
    NEWARRAY0 = 0xC2,
    /// Create an array with specified size.
    NEWARRAY = 0xC3,
    /// Create a typed array with specified size.
    NEWARRAY_T = 0xC4,
    /// Create an empty struct.
    NEWSTRUCT0 = 0xC5,
    /// Create a struct with specified size.
    NEWSTRUCT = 0xC6,
    /// Create an empty map.
    NEWMAP = 0xC8,
    /// Get the size of an array, map, or buffer.
    SIZE = 0xCA,
    /// Check if a key exists in a map or array.
    HASKEY = 0xCB,
    /// Get all keys from a map.
    KEYS = 0xCC,
    /// Get all values from a map or array.
    VALUES = 0xCD,
    /// Get an item from an array or map by index/key.
    PICKITEM = 0xCE,
    /// Append an item to an array.
    APPEND = 0xCF,
    /// Set an item in an array or map.
    SETITEM = 0xD0,
    /// Reverse the items in an array.
    REVERSEITEMS = 0xD1,
    /// Remove an item from an array or map.
    REMOVE = 0xD2,
    /// Clear all items from an array or map.
    CLEARITEMS = 0xD3,
    /// Pop the last item from an array.
    POPITEM = 0xD4,

    // ==================== Type Operations ====================
    /// Check if the top item is null.
    ISNULL = 0xD8,
    /// Check if the top item is of a specific type.
    ISTYPE = 0xD9,
    /// Convert the top item to a specific type.
    CONVERT = 0xDB,

    // ==================== Extension Operations ====================
    /// Abort execution with a custom message.
    ABORTMSG = 0xE0,
    /// Assert with a custom message on failure.
    ASSERTMSG = 0xE1,
}

impl OpCode {
    /// Returns an iterator over all `OpCode` variants.
    pub fn iter() -> impl Iterator<Item = Self> {
        // Create a vector of all OpCode variants
        // This matches the C# Neo implementation exactly
        vec![
            Self::PUSHINT8,
            Self::PUSHINT16,
            Self::PUSHINT32,
            Self::PUSHINT64,
            Self::PUSHINT128,
            Self::PUSHINT256,
            Self::PUSHT,
            Self::PUSHF,
            Self::PUSHA,
            Self::PUSHNULL,
            Self::PUSHDATA1,
            Self::PUSHDATA2,
            Self::PUSHDATA4,
            Self::PUSHM1,
            Self::PUSH0,
            Self::PUSH1,
            Self::PUSH2,
            Self::PUSH3,
            Self::PUSH4,
            Self::PUSH5,
            Self::PUSH6,
            Self::PUSH7,
            Self::PUSH8,
            Self::PUSH9,
            Self::PUSH10,
            Self::PUSH11,
            Self::PUSH12,
            Self::PUSH13,
            Self::PUSH14,
            Self::PUSH15,
            Self::PUSH16,
            Self::NOP,
            Self::JMP,
            Self::JMP_L,
            Self::JMPIF,
            Self::JMPIF_L,
            Self::JMPIFNOT,
            Self::JMPIFNOT_L,
            Self::JMPEQ,
            Self::JMPEQ_L,
            Self::JMPNE,
            Self::JMPNE_L,
            Self::JMPGT,
            Self::JMPGT_L,
            Self::JMPGE,
            Self::JMPGE_L,
            Self::JMPLT,
            Self::JMPLT_L,
            Self::JMPLE,
            Self::JMPLE_L,
            Self::CALL,
            Self::CALL_L,
            Self::CALLA,
            Self::CALLT,
            Self::ABORT,
            Self::ASSERT,
            Self::THROW,
            Self::TRY,
            Self::TRY_L,
            Self::ENDTRY,
            Self::ENDTRY_L,
            Self::ENDFINALLY,
            Self::RET,
            Self::SYSCALL,
            Self::DEPTH,
            Self::DROP,
            Self::NIP,
            Self::XDROP,
            Self::CLEAR,
            Self::DUP,
            Self::OVER,
            // TOALTSTACK removed - not in C# Neo
            Self::PICK,
            Self::TUCK,
            // FROMALTSTACK removed - not in C# Neo
            Self::SWAP,
            Self::ROT,
            Self::ROLL,
            Self::REVERSE3,
            Self::REVERSE4,
            Self::REVERSEN,
            Self::INITSSLOT,
            Self::INITSLOT,
            Self::LDSFLD0,
            Self::LDSFLD1,
            Self::LDSFLD2,
            Self::LDSFLD3,
            Self::LDSFLD4,
            Self::LDSFLD5,
            Self::LDSFLD6,
            Self::LDSFLD,
            Self::STSFLD0,
            Self::STSFLD1,
            Self::STSFLD2,
            Self::STSFLD3,
            Self::STSFLD4,
            Self::STSFLD5,
            Self::STSFLD6,
            Self::STSFLD,
            Self::LDLOC0,
            Self::LDLOC1,
            Self::LDLOC2,
            Self::LDLOC3,
            Self::LDLOC4,
            Self::LDLOC5,
            Self::LDLOC6,
            Self::LDLOC,
            Self::STLOC0,
            Self::STLOC1,
            Self::STLOC2,
            Self::STLOC3,
            Self::STLOC4,
            Self::STLOC5,
            Self::STLOC6,
            Self::STLOC,
            Self::LDARG0,
            Self::LDARG1,
            Self::LDARG2,
            Self::LDARG3,
            Self::LDARG4,
            Self::LDARG5,
            Self::LDARG6,
            Self::LDARG,
            Self::STARG0,
            Self::STARG1,
            Self::STARG2,
            Self::STARG3,
            Self::STARG4,
            Self::STARG5,
            Self::STARG6,
            Self::STARG,
            Self::NEWBUFFER,
            Self::MEMCPY,
            Self::CAT,
            Self::SUBSTR,
            Self::LEFT,
            Self::RIGHT,
            Self::INVERT,
            Self::AND,
            Self::OR,
            Self::XOR,
            Self::EQUAL,
            Self::NOTEQUAL,
            Self::SIGN,
            Self::ABS,
            Self::NEGATE,
            Self::INC,
            Self::DEC,
            Self::ADD,
            Self::SUB,
            Self::MUL,
            Self::DIV,
            Self::MOD,
            Self::POW,
            Self::SQRT,
            Self::MODMUL,
            Self::MODPOW,
            Self::SHL,
            Self::SHR,
            Self::NOT,
            Self::BOOLAND,
            Self::BOOLOR,
            Self::NZ,
            Self::NUMEQUAL,
            Self::NUMNOTEQUAL,
            Self::LT,
            Self::LE,
            Self::GT,
            Self::GE,
            Self::MIN,
            Self::MAX,
            Self::WITHIN,
            Self::PACKMAP,
            Self::PACKSTRUCT,
            Self::PACK,
            Self::UNPACK,
            Self::NEWARRAY0,
            Self::NEWARRAY,
            Self::NEWARRAY_T,
            Self::NEWSTRUCT0,
            Self::NEWSTRUCT,
            Self::NEWMAP,
            Self::SIZE,
            Self::HASKEY,
            Self::KEYS,
            Self::VALUES,
            Self::PICKITEM,
            Self::APPEND,
            Self::SETITEM,
            Self::REVERSEITEMS,
            Self::REMOVE,
            Self::CLEARITEMS,
            Self::POPITEM,
            Self::ISNULL,
            Self::ISTYPE,
            Self::CONVERT,
            Self::ABORTMSG,
            Self::ASSERTMSG,
        ]
        .into_iter()
    }

    /// Creates an `OpCode` from a byte value.
    #[must_use]
    pub const fn from_byte(byte: u8) -> Option<Self> {
        match byte {
            0x00 => Some(Self::PUSHINT8),
            0x01 => Some(Self::PUSHINT16),
            0x02 => Some(Self::PUSHINT32),
            0x03 => Some(Self::PUSHINT64),
            0x04 => Some(Self::PUSHINT128),
            0x05 => Some(Self::PUSHINT256),
            0x08 => Some(Self::PUSHT),
            0x09 => Some(Self::PUSHF),
            0x0A => Some(Self::PUSHA),
            0x0B => Some(Self::PUSHNULL),
            0x0C => Some(Self::PUSHDATA1),
            0x0D => Some(Self::PUSHDATA2),
            0x0E => Some(Self::PUSHDATA4),
            0x0F => Some(Self::PUSHM1),
            0x10 => Some(Self::PUSH0),
            0x11 => Some(Self::PUSH1),
            0x12 => Some(Self::PUSH2),
            0x13 => Some(Self::PUSH3),
            0x14 => Some(Self::PUSH4),
            0x15 => Some(Self::PUSH5),
            0x16 => Some(Self::PUSH6),
            0x17 => Some(Self::PUSH7),
            0x18 => Some(Self::PUSH8),
            0x19 => Some(Self::PUSH9),
            0x1A => Some(Self::PUSH10),
            0x1B => Some(Self::PUSH11),
            0x1C => Some(Self::PUSH12),
            0x1D => Some(Self::PUSH13),
            0x1E => Some(Self::PUSH14),
            0x1F => Some(Self::PUSH15),
            0x20 => Some(Self::PUSH16),
            0x21 => Some(Self::NOP),
            0x22 => Some(Self::JMP),
            0x23 => Some(Self::JMP_L),
            0x24 => Some(Self::JMPIF),
            0x25 => Some(Self::JMPIF_L),
            0x26 => Some(Self::JMPIFNOT),
            0x27 => Some(Self::JMPIFNOT_L),
            0x28 => Some(Self::JMPEQ),
            0x29 => Some(Self::JMPEQ_L),
            0x2A => Some(Self::JMPNE),
            0x2B => Some(Self::JMPNE_L),
            0x2C => Some(Self::JMPGT),
            0x2D => Some(Self::JMPGT_L),
            0x2E => Some(Self::JMPGE),
            0x2F => Some(Self::JMPGE_L),
            0x30 => Some(Self::JMPLT),
            0x31 => Some(Self::JMPLT_L),
            0x32 => Some(Self::JMPLE),
            0x33 => Some(Self::JMPLE_L),
            0x34 => Some(Self::CALL),
            0x35 => Some(Self::CALL_L),
            0x36 => Some(Self::CALLA),
            0x37 => Some(Self::CALLT),
            0x38 => Some(Self::ABORT),
            0x39 => Some(Self::ASSERT),
            0x3A => Some(Self::THROW),
            0x3B => Some(Self::TRY),
            0x3C => Some(Self::TRY_L),
            0x3D => Some(Self::ENDTRY),
            0x3E => Some(Self::ENDTRY_L),
            0x3F => Some(Self::ENDFINALLY),
            0x40 => Some(Self::RET),
            0x41 => Some(Self::SYSCALL),
            0x43 => Some(Self::DEPTH),
            0x45 => Some(Self::DROP),
            0x46 => Some(Self::NIP),
            0x48 => Some(Self::XDROP),
            0x49 => Some(Self::CLEAR),
            0x4A => Some(Self::DUP),
            0x4B => Some(Self::OVER),
            // 0x4C TOALTSTACK not in C# Neo
            0x4D => Some(Self::PICK),
            0x4E => Some(Self::TUCK),
            // 0x4F FROMALTSTACK not in C# Neo
            0x50 => Some(Self::SWAP),
            0x51 => Some(Self::ROT),
            0x52 => Some(Self::ROLL),
            0x53 => Some(Self::REVERSE3),
            0x54 => Some(Self::REVERSE4),
            0x55 => Some(Self::REVERSEN),
            0x56 => Some(Self::INITSSLOT),
            0x57 => Some(Self::INITSLOT),
            0x58 => Some(Self::LDSFLD0),
            0x59 => Some(Self::LDSFLD1),
            0x5A => Some(Self::LDSFLD2),
            0x5B => Some(Self::LDSFLD3),
            0x5C => Some(Self::LDSFLD4),
            0x5D => Some(Self::LDSFLD5),
            0x5E => Some(Self::LDSFLD6),
            0x5F => Some(Self::LDSFLD),
            0x60 => Some(Self::STSFLD0),
            0x61 => Some(Self::STSFLD1),
            0x62 => Some(Self::STSFLD2),
            0x63 => Some(Self::STSFLD3),
            0x64 => Some(Self::STSFLD4),
            0x65 => Some(Self::STSFLD5),
            0x66 => Some(Self::STSFLD6),
            0x67 => Some(Self::STSFLD),
            0x68 => Some(Self::LDLOC0),
            0x69 => Some(Self::LDLOC1),
            0x6A => Some(Self::LDLOC2),
            0x6B => Some(Self::LDLOC3),
            0x6C => Some(Self::LDLOC4),
            0x6D => Some(Self::LDLOC5),
            0x6E => Some(Self::LDLOC6),
            0x6F => Some(Self::LDLOC),
            0x70 => Some(Self::STLOC0),
            0x71 => Some(Self::STLOC1),
            0x72 => Some(Self::STLOC2),
            0x73 => Some(Self::STLOC3),
            0x74 => Some(Self::STLOC4),
            0x75 => Some(Self::STLOC5),
            0x76 => Some(Self::STLOC6),
            0x77 => Some(Self::STLOC),
            0x78 => Some(Self::LDARG0),
            0x79 => Some(Self::LDARG1),
            0x7A => Some(Self::LDARG2),
            0x7B => Some(Self::LDARG3),
            0x7C => Some(Self::LDARG4),
            0x7D => Some(Self::LDARG5),
            0x7E => Some(Self::LDARG6),
            0x7F => Some(Self::LDARG),
            0x80 => Some(Self::STARG0),
            0x81 => Some(Self::STARG1),
            0x82 => Some(Self::STARG2),
            0x83 => Some(Self::STARG3),
            0x84 => Some(Self::STARG4),
            0x85 => Some(Self::STARG5),
            0x86 => Some(Self::STARG6),
            0x87 => Some(Self::STARG),
            0x88 => Some(Self::NEWBUFFER),
            0x89 => Some(Self::MEMCPY),
            // 0x8A is not used in C# Neo
            0x8B => Some(Self::CAT),
            0x8C => Some(Self::SUBSTR),
            0x8D => Some(Self::LEFT),
            0x8E => Some(Self::RIGHT),
            0x90 => Some(Self::INVERT),
            0x91 => Some(Self::AND),
            0x92 => Some(Self::OR),
            0x93 => Some(Self::XOR),
            0x97 => Some(Self::EQUAL),
            0x98 => Some(Self::NOTEQUAL),
            0x99 => Some(Self::SIGN),
            0x9A => Some(Self::ABS),
            0x9B => Some(Self::NEGATE),
            0x9C => Some(Self::INC),
            0x9D => Some(Self::DEC),
            0x9E => Some(Self::ADD),
            0x9F => Some(Self::SUB),
            0xA0 => Some(Self::MUL),
            0xA1 => Some(Self::DIV),
            0xA2 => Some(Self::MOD),
            0xA3 => Some(Self::POW),
            0xA4 => Some(Self::SQRT),
            0xA5 => Some(Self::MODMUL),
            0xA6 => Some(Self::MODPOW),
            0xA8 => Some(Self::SHL),
            0xA9 => Some(Self::SHR),
            0xAA => Some(Self::NOT),
            0xAB => Some(Self::BOOLAND),
            0xAC => Some(Self::BOOLOR),
            0xB1 => Some(Self::NZ),
            0xB3 => Some(Self::NUMEQUAL),
            0xB4 => Some(Self::NUMNOTEQUAL),
            0xB5 => Some(Self::LT),
            0xB6 => Some(Self::LE),
            0xB7 => Some(Self::GT),
            0xB8 => Some(Self::GE),
            0xB9 => Some(Self::MIN),
            0xBA => Some(Self::MAX),
            0xBB => Some(Self::WITHIN),
            0xBE => Some(Self::PACKMAP),
            0xBF => Some(Self::PACKSTRUCT),
            0xC0 => Some(Self::PACK),
            0xC1 => Some(Self::UNPACK),
            0xC2 => Some(Self::NEWARRAY0),
            0xC3 => Some(Self::NEWARRAY),
            0xC4 => Some(Self::NEWARRAY_T),
            0xC5 => Some(Self::NEWSTRUCT0),
            0xC6 => Some(Self::NEWSTRUCT),
            0xC8 => Some(Self::NEWMAP),
            0xCA => Some(Self::SIZE),
            0xCB => Some(Self::HASKEY),
            0xCC => Some(Self::KEYS),
            0xCD => Some(Self::VALUES),
            0xCE => Some(Self::PICKITEM),
            0xCF => Some(Self::APPEND),
            0xD0 => Some(Self::SETITEM),
            0xD1 => Some(Self::REVERSEITEMS),
            0xD2 => Some(Self::REMOVE),
            0xD3 => Some(Self::CLEARITEMS),
            0xD4 => Some(Self::POPITEM),
            0xD8 => Some(Self::ISNULL),
            0xD9 => Some(Self::ISTYPE),
            0xDB => Some(Self::CONVERT),
            0xE0 => Some(Self::ABORTMSG),
            0xE1 => Some(Self::ASSERTMSG),
            _ => None,
        }
    }

    /// Returns the operand size for this opcode.
    #[must_use]
    pub fn operand_size(&self) -> OperandSize {
        match self {
            Self::PUSHINT8 => OperandSize::fixed(1),
            Self::PUSHINT16 => OperandSize::fixed(2),
            Self::PUSHINT32 => OperandSize::fixed(4),
            Self::PUSHINT64 => OperandSize::fixed(8),
            Self::PUSHINT128 => OperandSize::fixed(16),
            Self::PUSHINT256 => OperandSize::fixed(HASH_SIZE as i32),
            Self::PUSHA => OperandSize::fixed(4),
            Self::PUSHDATA1 => OperandSize::prefix(1),
            Self::PUSHDATA2 => OperandSize::prefix(2),
            Self::PUSHDATA4 => OperandSize::prefix(4),
            Self::JMP => OperandSize::fixed(1),
            Self::JMP_L => OperandSize::fixed(4),
            Self::JMPIF => OperandSize::fixed(1),
            Self::JMPIF_L => OperandSize::fixed(4),
            Self::JMPIFNOT => OperandSize::fixed(1),
            Self::JMPIFNOT_L => OperandSize::fixed(4),
            Self::JMPEQ => OperandSize::fixed(1),
            Self::JMPEQ_L => OperandSize::fixed(4),
            Self::JMPNE => OperandSize::fixed(1),
            Self::JMPNE_L => OperandSize::fixed(4),
            Self::JMPGT => OperandSize::fixed(1),
            Self::JMPGT_L => OperandSize::fixed(4),
            Self::JMPGE => OperandSize::fixed(1),
            Self::JMPGE_L => OperandSize::fixed(4),
            Self::JMPLT => OperandSize::fixed(1),
            Self::JMPLT_L => OperandSize::fixed(4),
            Self::JMPLE => OperandSize::fixed(1),
            Self::JMPLE_L => OperandSize::fixed(4),
            Self::CALL => OperandSize::fixed(1),
            Self::CALL_L => OperandSize::fixed(4),
            Self::CALLT => OperandSize::fixed(2),
            Self::TRY => OperandSize::fixed(2),
            Self::TRY_L => OperandSize::fixed(8),
            Self::ENDTRY => OperandSize::fixed(1),
            Self::ENDTRY_L => OperandSize::fixed(4),
            Self::SYSCALL => OperandSize::fixed(4),
            Self::INITSSLOT => OperandSize::fixed(1),
            Self::INITSLOT => OperandSize::fixed(2),
            Self::LDSFLD => OperandSize::fixed(1),
            Self::STSFLD => OperandSize::fixed(1),
            Self::LDLOC => OperandSize::fixed(1),
            Self::STLOC => OperandSize::fixed(1),
            Self::LDARG => OperandSize::fixed(1),
            Self::STARG => OperandSize::fixed(1),
            Self::NEWARRAY_T => OperandSize::fixed(1),
            Self::ISTYPE => OperandSize::fixed(1),
            Self::CONVERT => OperandSize::fixed(1),
            _ => OperandSize::fixed(0),
        }
    }
}

impl TryFrom<u8> for OpCode {
    type Error = crate::VmError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_byte(value).ok_or_else(|| crate::VmError::invalid_opcode(value))
    }
}
