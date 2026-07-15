//! Canonical NeoVM opcode metadata.

macro_rules! count_opcodes {
    ($($name:ident),+ $(,)?) => {
        <[()]>::len(&[$(count_opcodes!(@unit $name)),+])
    };
    (@unit $name:ident) => {
        ()
    };
}

macro_rules! define_opcodes {
    ($(
        $name:ident = $byte:expr, operand_size = $operand_size:expr, operand_prefix = $operand_prefix:expr;
    )+) => {
        /// NeoVM operation codes.
        ///
        /// Values follow the canonical Neo N3 opcode assignment used by Neo.VM 3.10.1.
        /// Execution engines may support additional host-level pseudo operations, but
        /// shared bytecode decoding should use this table.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        #[allow(missing_docs, non_camel_case_types)]
        pub enum OpCode {
            $(
                $name = $byte,
            )+
        }

        impl OpCode {
            /// All canonical opcodes accepted by this shared metadata table.
            pub const ALL: [Self; count_opcodes!($($name),+)] = [
                $(
                    Self::$name,
                )+
            ];

            const LOOKUP: [Option<Self>; 256] = Self::build_lookup();

            const fn build_lookup() -> [Option<Self>; 256] {
                let mut lookup = [None; 256];
                let mut index = 0;
                while index < Self::ALL.len() {
                    let opcode = Self::ALL[index];
                    lookup[opcode as usize] = Some(opcode);
                    index += 1;
                }
                lookup
            }

            /// Convert a byte to an opcode.
            #[must_use]
            pub fn from_u8(value: u8) -> Option<Self> {
                Self::try_from(value).ok()
            }

            /// Convert a canonical opcode name to an opcode.
            ///
            /// The lookup is ASCII case-insensitive so tools can accept assembler-style
            /// input without carrying their own opcode string tables.
            #[must_use]
            pub fn from_name(name: &str) -> Option<Self> {
                Self::ALL
                    .iter()
                    .copied()
                    .find(|opcode| opcode.name().eq_ignore_ascii_case(name))
            }

            /// Return the opcode byte.
            #[must_use]
            pub const fn byte(self) -> u8 {
                self as u8
            }

            /// Returns the number of fixed operand bytes that follow this opcode.
            #[must_use]
            pub const fn operand_size(self) -> usize {
                match self {
                    $(
                        Self::$name => $operand_size,
                    )+
                }
            }

            /// Returns the variable-length operand prefix byte count.
            #[must_use]
            pub const fn operand_prefix(self) -> usize {
                match self {
                    $(
                        Self::$name => $operand_prefix,
                    )+
                }
            }

            /// Returns the canonical opcode name.
            #[must_use]
            pub const fn name(self) -> &'static str {
                match self {
                    $(
                        Self::$name => stringify!($name),
                    )+
                }
            }
        }

        impl TryFrom<u8> for OpCode {
            type Error = u8;

            fn try_from(value: u8) -> Result<Self, Self::Error> {
                Self::LOOKUP[value as usize].ok_or(value)
            }
        }

        impl core::fmt::Display for OpCode {
            fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
                f.write_str(self.name())
            }
        }
    };
}

define_opcodes! {
    PUSHINT8 = 0x00, operand_size = 1, operand_prefix = 0;
    PUSHINT16 = 0x01, operand_size = 2, operand_prefix = 0;
    PUSHINT32 = 0x02, operand_size = 4, operand_prefix = 0;
    PUSHINT64 = 0x03, operand_size = 8, operand_prefix = 0;
    PUSHINT128 = 0x04, operand_size = 16, operand_prefix = 0;
    PUSHINT256 = 0x05, operand_size = 32, operand_prefix = 0;
    PUSHT = 0x08, operand_size = 0, operand_prefix = 0;
    PUSHF = 0x09, operand_size = 0, operand_prefix = 0;
    PUSHA = 0x0A, operand_size = 4, operand_prefix = 0;
    PUSHNULL = 0x0B, operand_size = 0, operand_prefix = 0;
    PUSHDATA1 = 0x0C, operand_size = 1, operand_prefix = 1;
    PUSHDATA2 = 0x0D, operand_size = 2, operand_prefix = 2;
    PUSHDATA4 = 0x0E, operand_size = 4, operand_prefix = 4;
    PUSHM1 = 0x0F, operand_size = 0, operand_prefix = 0;
    PUSH0 = 0x10, operand_size = 0, operand_prefix = 0;
    PUSH1 = 0x11, operand_size = 0, operand_prefix = 0;
    PUSH2 = 0x12, operand_size = 0, operand_prefix = 0;
    PUSH3 = 0x13, operand_size = 0, operand_prefix = 0;
    PUSH4 = 0x14, operand_size = 0, operand_prefix = 0;
    PUSH5 = 0x15, operand_size = 0, operand_prefix = 0;
    PUSH6 = 0x16, operand_size = 0, operand_prefix = 0;
    PUSH7 = 0x17, operand_size = 0, operand_prefix = 0;
    PUSH8 = 0x18, operand_size = 0, operand_prefix = 0;
    PUSH9 = 0x19, operand_size = 0, operand_prefix = 0;
    PUSH10 = 0x1A, operand_size = 0, operand_prefix = 0;
    PUSH11 = 0x1B, operand_size = 0, operand_prefix = 0;
    PUSH12 = 0x1C, operand_size = 0, operand_prefix = 0;
    PUSH13 = 0x1D, operand_size = 0, operand_prefix = 0;
    PUSH14 = 0x1E, operand_size = 0, operand_prefix = 0;
    PUSH15 = 0x1F, operand_size = 0, operand_prefix = 0;
    PUSH16 = 0x20, operand_size = 0, operand_prefix = 0;
    NOP = 0x21, operand_size = 0, operand_prefix = 0;
    JMP = 0x22, operand_size = 1, operand_prefix = 0;
    JMP_L = 0x23, operand_size = 4, operand_prefix = 0;
    JMPIF = 0x24, operand_size = 1, operand_prefix = 0;
    JMPIF_L = 0x25, operand_size = 4, operand_prefix = 0;
    JMPIFNOT = 0x26, operand_size = 1, operand_prefix = 0;
    JMPIFNOT_L = 0x27, operand_size = 4, operand_prefix = 0;
    JMPEQ = 0x28, operand_size = 1, operand_prefix = 0;
    JMPEQ_L = 0x29, operand_size = 4, operand_prefix = 0;
    JMPNE = 0x2A, operand_size = 1, operand_prefix = 0;
    JMPNE_L = 0x2B, operand_size = 4, operand_prefix = 0;
    JMPGT = 0x2C, operand_size = 1, operand_prefix = 0;
    JMPGT_L = 0x2D, operand_size = 4, operand_prefix = 0;
    JMPGE = 0x2E, operand_size = 1, operand_prefix = 0;
    JMPGE_L = 0x2F, operand_size = 4, operand_prefix = 0;
    JMPLT = 0x30, operand_size = 1, operand_prefix = 0;
    JMPLT_L = 0x31, operand_size = 4, operand_prefix = 0;
    JMPLE = 0x32, operand_size = 1, operand_prefix = 0;
    JMPLE_L = 0x33, operand_size = 4, operand_prefix = 0;
    CALL = 0x34, operand_size = 1, operand_prefix = 0;
    CALL_L = 0x35, operand_size = 4, operand_prefix = 0;
    CALLA = 0x36, operand_size = 0, operand_prefix = 0;
    CALLT = 0x37, operand_size = 2, operand_prefix = 0;
    ABORT = 0x38, operand_size = 0, operand_prefix = 0;
    ASSERT = 0x39, operand_size = 0, operand_prefix = 0;
    THROW = 0x3A, operand_size = 0, operand_prefix = 0;
    TRY = 0x3B, operand_size = 2, operand_prefix = 0;
    TRY_L = 0x3C, operand_size = 8, operand_prefix = 0;
    ENDTRY = 0x3D, operand_size = 1, operand_prefix = 0;
    ENDTRY_L = 0x3E, operand_size = 4, operand_prefix = 0;
    ENDFINALLY = 0x3F, operand_size = 0, operand_prefix = 0;
    RET = 0x40, operand_size = 0, operand_prefix = 0;
    SYSCALL = 0x41, operand_size = 4, operand_prefix = 0;
    DEPTH = 0x43, operand_size = 0, operand_prefix = 0;
    DROP = 0x45, operand_size = 0, operand_prefix = 0;
    NIP = 0x46, operand_size = 0, operand_prefix = 0;
    XDROP = 0x48, operand_size = 0, operand_prefix = 0;
    CLEAR = 0x49, operand_size = 0, operand_prefix = 0;
    DUP = 0x4A, operand_size = 0, operand_prefix = 0;
    OVER = 0x4B, operand_size = 0, operand_prefix = 0;
    PICK = 0x4D, operand_size = 0, operand_prefix = 0;
    TUCK = 0x4E, operand_size = 0, operand_prefix = 0;
    SWAP = 0x50, operand_size = 0, operand_prefix = 0;
    ROT = 0x51, operand_size = 0, operand_prefix = 0;
    ROLL = 0x52, operand_size = 0, operand_prefix = 0;
    REVERSE3 = 0x53, operand_size = 0, operand_prefix = 0;
    REVERSE4 = 0x54, operand_size = 0, operand_prefix = 0;
    REVERSEN = 0x55, operand_size = 0, operand_prefix = 0;
    INITSSLOT = 0x56, operand_size = 1, operand_prefix = 0;
    INITSLOT = 0x57, operand_size = 2, operand_prefix = 0;
    LDSFLD0 = 0x58, operand_size = 0, operand_prefix = 0;
    LDSFLD1 = 0x59, operand_size = 0, operand_prefix = 0;
    LDSFLD2 = 0x5A, operand_size = 0, operand_prefix = 0;
    LDSFLD3 = 0x5B, operand_size = 0, operand_prefix = 0;
    LDSFLD4 = 0x5C, operand_size = 0, operand_prefix = 0;
    LDSFLD5 = 0x5D, operand_size = 0, operand_prefix = 0;
    LDSFLD6 = 0x5E, operand_size = 0, operand_prefix = 0;
    LDSFLD = 0x5F, operand_size = 1, operand_prefix = 0;
    STSFLD0 = 0x60, operand_size = 0, operand_prefix = 0;
    STSFLD1 = 0x61, operand_size = 0, operand_prefix = 0;
    STSFLD2 = 0x62, operand_size = 0, operand_prefix = 0;
    STSFLD3 = 0x63, operand_size = 0, operand_prefix = 0;
    STSFLD4 = 0x64, operand_size = 0, operand_prefix = 0;
    STSFLD5 = 0x65, operand_size = 0, operand_prefix = 0;
    STSFLD6 = 0x66, operand_size = 0, operand_prefix = 0;
    STSFLD = 0x67, operand_size = 1, operand_prefix = 0;
    LDLOC0 = 0x68, operand_size = 0, operand_prefix = 0;
    LDLOC1 = 0x69, operand_size = 0, operand_prefix = 0;
    LDLOC2 = 0x6A, operand_size = 0, operand_prefix = 0;
    LDLOC3 = 0x6B, operand_size = 0, operand_prefix = 0;
    LDLOC4 = 0x6C, operand_size = 0, operand_prefix = 0;
    LDLOC5 = 0x6D, operand_size = 0, operand_prefix = 0;
    LDLOC6 = 0x6E, operand_size = 0, operand_prefix = 0;
    LDLOC = 0x6F, operand_size = 1, operand_prefix = 0;
    STLOC0 = 0x70, operand_size = 0, operand_prefix = 0;
    STLOC1 = 0x71, operand_size = 0, operand_prefix = 0;
    STLOC2 = 0x72, operand_size = 0, operand_prefix = 0;
    STLOC3 = 0x73, operand_size = 0, operand_prefix = 0;
    STLOC4 = 0x74, operand_size = 0, operand_prefix = 0;
    STLOC5 = 0x75, operand_size = 0, operand_prefix = 0;
    STLOC6 = 0x76, operand_size = 0, operand_prefix = 0;
    STLOC = 0x77, operand_size = 1, operand_prefix = 0;
    LDARG0 = 0x78, operand_size = 0, operand_prefix = 0;
    LDARG1 = 0x79, operand_size = 0, operand_prefix = 0;
    LDARG2 = 0x7A, operand_size = 0, operand_prefix = 0;
    LDARG3 = 0x7B, operand_size = 0, operand_prefix = 0;
    LDARG4 = 0x7C, operand_size = 0, operand_prefix = 0;
    LDARG5 = 0x7D, operand_size = 0, operand_prefix = 0;
    LDARG6 = 0x7E, operand_size = 0, operand_prefix = 0;
    LDARG = 0x7F, operand_size = 1, operand_prefix = 0;
    STARG0 = 0x80, operand_size = 0, operand_prefix = 0;
    STARG1 = 0x81, operand_size = 0, operand_prefix = 0;
    STARG2 = 0x82, operand_size = 0, operand_prefix = 0;
    STARG3 = 0x83, operand_size = 0, operand_prefix = 0;
    STARG4 = 0x84, operand_size = 0, operand_prefix = 0;
    STARG5 = 0x85, operand_size = 0, operand_prefix = 0;
    STARG6 = 0x86, operand_size = 0, operand_prefix = 0;
    STARG = 0x87, operand_size = 1, operand_prefix = 0;
    NEWBUFFER = 0x88, operand_size = 0, operand_prefix = 0;
    MEMCPY = 0x89, operand_size = 0, operand_prefix = 0;
    CAT = 0x8B, operand_size = 0, operand_prefix = 0;
    SUBSTR = 0x8C, operand_size = 0, operand_prefix = 0;
    LEFT = 0x8D, operand_size = 0, operand_prefix = 0;
    RIGHT = 0x8E, operand_size = 0, operand_prefix = 0;
    INVERT = 0x90, operand_size = 0, operand_prefix = 0;
    AND = 0x91, operand_size = 0, operand_prefix = 0;
    OR = 0x92, operand_size = 0, operand_prefix = 0;
    XOR = 0x93, operand_size = 0, operand_prefix = 0;
    EQUAL = 0x97, operand_size = 0, operand_prefix = 0;
    NOTEQUAL = 0x98, operand_size = 0, operand_prefix = 0;
    SIGN = 0x99, operand_size = 0, operand_prefix = 0;
    ABS = 0x9A, operand_size = 0, operand_prefix = 0;
    NEGATE = 0x9B, operand_size = 0, operand_prefix = 0;
    INC = 0x9C, operand_size = 0, operand_prefix = 0;
    DEC = 0x9D, operand_size = 0, operand_prefix = 0;
    ADD = 0x9E, operand_size = 0, operand_prefix = 0;
    SUB = 0x9F, operand_size = 0, operand_prefix = 0;
    MUL = 0xA0, operand_size = 0, operand_prefix = 0;
    DIV = 0xA1, operand_size = 0, operand_prefix = 0;
    MOD = 0xA2, operand_size = 0, operand_prefix = 0;
    POW = 0xA3, operand_size = 0, operand_prefix = 0;
    SQRT = 0xA4, operand_size = 0, operand_prefix = 0;
    MODMUL = 0xA5, operand_size = 0, operand_prefix = 0;
    MODPOW = 0xA6, operand_size = 0, operand_prefix = 0;
    SHL = 0xA8, operand_size = 0, operand_prefix = 0;
    SHR = 0xA9, operand_size = 0, operand_prefix = 0;
    NOT = 0xAA, operand_size = 0, operand_prefix = 0;
    BOOLAND = 0xAB, operand_size = 0, operand_prefix = 0;
    BOOLOR = 0xAC, operand_size = 0, operand_prefix = 0;
    NZ = 0xB1, operand_size = 0, operand_prefix = 0;
    NUMEQUAL = 0xB3, operand_size = 0, operand_prefix = 0;
    NUMNOTEQUAL = 0xB4, operand_size = 0, operand_prefix = 0;
    LT = 0xB5, operand_size = 0, operand_prefix = 0;
    LE = 0xB6, operand_size = 0, operand_prefix = 0;
    GT = 0xB7, operand_size = 0, operand_prefix = 0;
    GE = 0xB8, operand_size = 0, operand_prefix = 0;
    MIN = 0xB9, operand_size = 0, operand_prefix = 0;
    MAX = 0xBA, operand_size = 0, operand_prefix = 0;
    WITHIN = 0xBB, operand_size = 0, operand_prefix = 0;
    PACKMAP = 0xBE, operand_size = 0, operand_prefix = 0;
    PACKSTRUCT = 0xBF, operand_size = 0, operand_prefix = 0;
    PACK = 0xC0, operand_size = 0, operand_prefix = 0;
    UNPACK = 0xC1, operand_size = 0, operand_prefix = 0;
    NEWARRAY0 = 0xC2, operand_size = 0, operand_prefix = 0;
    NEWARRAY = 0xC3, operand_size = 0, operand_prefix = 0;
    NEWARRAY_T = 0xC4, operand_size = 1, operand_prefix = 0;
    NEWSTRUCT0 = 0xC5, operand_size = 0, operand_prefix = 0;
    NEWSTRUCT = 0xC6, operand_size = 0, operand_prefix = 0;
    NEWMAP = 0xC8, operand_size = 0, operand_prefix = 0;
    SIZE = 0xCA, operand_size = 0, operand_prefix = 0;
    HASKEY = 0xCB, operand_size = 0, operand_prefix = 0;
    KEYS = 0xCC, operand_size = 0, operand_prefix = 0;
    VALUES = 0xCD, operand_size = 0, operand_prefix = 0;
    PICKITEM = 0xCE, operand_size = 0, operand_prefix = 0;
    APPEND = 0xCF, operand_size = 0, operand_prefix = 0;
    SETITEM = 0xD0, operand_size = 0, operand_prefix = 0;
    REVERSEITEMS = 0xD1, operand_size = 0, operand_prefix = 0;
    REMOVE = 0xD2, operand_size = 0, operand_prefix = 0;
    CLEARITEMS = 0xD3, operand_size = 0, operand_prefix = 0;
    POPITEM = 0xD4, operand_size = 0, operand_prefix = 0;
    ISNULL = 0xD8, operand_size = 0, operand_prefix = 0;
    ISTYPE = 0xD9, operand_size = 1, operand_prefix = 0;
    CONVERT = 0xDB, operand_size = 1, operand_prefix = 0;
    ABORTMSG = 0xE0, operand_size = 0, operand_prefix = 0;
    ASSERTMSG = 0xE1, operand_size = 0, operand_prefix = 0;
}
