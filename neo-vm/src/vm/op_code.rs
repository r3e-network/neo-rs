use std::{collections::HashMap, fmt::Error};
use lazy_static::lazy_static;
use num_enum::FromPrimitive;
use crate::vm::vm_error::VMError;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, FromPrimitive)]
pub enum OpCode {
	PushInt8 = 0x00,
	PushInt16 = 0x01,
	PushInt32 = 0x02,
	PushInt64 = 0x03,
	PushInt128 = 0x04,
	PushInt256 = 0x05,
	PushTrue = 0x08,
	PushFalse = 0x09,
	PushA = 0x0A,
	PushNull = 0x0B,
	PushData1 = 0x0C,
	PushData2 = 0x0D,
	PushData4 = 0x0E,
	PushM1 = 0x0F,
	Push0 = 0x10,
	Push1 = 0x11,
	Push2 = 0x12,
	Push3 = 0x13,
	Push4 = 0x14,
	Push5 = 0x15,
	Push6 = 0x16,
	Push7 = 0x17,
	Push8 = 0x18,
	Push9 = 0x19,
	Push10 = 0x1A,
	Push11 = 0x1B,
	Push12 = 0x1C,
	Push13 = 0x1D,
	Push14 = 0x1E,
	Push15 = 0x1F,
	Push16 = 0x20,

	Nop = 0x21,
	Jmp = 0x22,
	JmpL = 0x23,
	JmpIf = 0x24,
	JmpIfL = 0x25,
	JmpIfNot = 0x26,
	JmpIfNotL = 0x27,
	JmpEq = 0x28,
	JmpEqL = 0x29,
	JmpNe = 0x2A,
	JmpNeL = 0x2B,
	JmpGt = 0x2C,
	JmpGtL = 0x2D,
	JmpGe = 0x2E,
	JmpGeL = 0x2F,
	JmpLt = 0x30,
	JmpLtL = 0x31,
	JmpLe = 0x32,
	JmpLeL = 0x33,
	Call = 0x34,
	CallL = 0x35,
	CallA = 0x36,
	CallT = 0x37,
	Abort = 0x38,
	Assert = 0x39,
	Throw = 0x3A,
	Try = 0x3B,
	TryL = 0x3C,
	EndTry = 0x3D,
	EndTryL = 0x3E,
	EndFinally = 0x3F,
	Ret = 0x40,
	Syscall = 0x41,

	Depth = 0x43,
	Drop = 0x45,
	Nip = 0x46,
	Xdrop = 0x48,
	Clear = 0x49,
	Dup = 0x4A,
	Over = 0x4B,
	Pick = 0x4D,
	Tuck = 0x4E,
	Swap = 0x50,
	Rot = 0x51,
	Roll = 0x52,
	Reverse3 = 0x53,
	Reverse4 = 0x54,
	ReverseN = 0x55,

	InitSSLot = 0x56,
	InitSlot = 0x57,
	LdSFLd0 = 0x58,
	LdSFLd1 = 0x59,
	LdSFLd2 = 0x5A,
	LdSFLd3 = 0x5B,
	LdSFLd4 = 0x5C,
	LdSFLd5 = 0x5D,
	LdSFLd6 = 0x5E,
	LdSFLd = 0x5F,
	StSFLd0 = 0x60,
	StSFLd1 = 0x61,
	StSFLd2 = 0x62,
	StSFLd3 = 0x63,
	StSFLd4 = 0x64,
	StSFLd5 = 0x65,
	StSFLd6 = 0x66,
	StSFLd = 0x67,
	LdLoc0 = 0x68,
	LdLoc1 = 0x69,
	LdLoc2 = 0x6A,
	LdLoc3 = 0x6B,
	LdLoc4 = 0x6C,
	LdLoc5 = 0x6D,
	LdLoc6 = 0x6E,
	LdLoc = 0x6F,
	StLoc0 = 0x70,
	StLoc1 = 0x71,
	StLoc2 = 0x72,
	StLoc3 = 0x73,
	StLoc4 = 0x74,
	StLoc5 = 0x75,
	StLoc6 = 0x76,
	StLoc = 0x77,
	LdArg0 = 0x78,
	LdArg1 = 0x79,
	LdArg2 = 0x7A,
	LdArg3 = 0x7B,
	LdArg4 = 0x7C,
	LdArg5 = 0x7D,
	LdArg6 = 0x7E,
	LdArg = 0x7F,
	StArg0 = 0x80,
	StArg1 = 0x81,
	StArg2 = 0x82,
	StArg3 = 0x83,
	StArg4 = 0x84,
	StArg5 = 0x85,
	StArg6 = 0x86,
	StArg = 0x87,

	NewBuffer = 0x88,
	MemCpy = 0x89,
	Cat = 0x8B,
	Substr = 0x8C,
	Left = 0x8D,
	Right = 0x8E,

	Invert = 0x90,
	And = 0x91,
	Or = 0x92,
	Xor = 0x93,
	Equal = 0x97,
	NotEqual = 0x98,

	Sign = 0x99,
	Abs = 0x9A,
	Negate = 0x9B,
	Inc = 0x9C,
	Dec = 0x9D,
	Add = 0x9E,
	Sub = 0x9F,
	Mul = 0xA0,
	Div = 0xA1,
	Mod = 0xA2,
	Pow = 0xA3,
	Sqrt = 0xA4,
	ModMul = 0xA5,
	ModPow = 0xA6,
	Shl = 0xA8,
	Shr = 0xA9,
	Not = 0xAA,
	BoolAnd = 0xAB,
	BoolOr = 0xAC,
	Nz = 0xB1,
	NumEqual = 0xB3,
	NumNotEqual = 0xB4,
	Lt = 0xB5,
	Le = 0xB6,
	Gt = 0xB7,
	Ge = 0xB8,
	Min = 0xB9,
	Max = 0xBA,
	Within = 0xBB,

	PackMap = 0xBE,
	PackStruct = 0xBF,
	Pack = 0xC0,
	Unpack = 0xC1,
	NewArray0 = 0xC2,
	NewArray = 0xC3,
	NewArrayT = 0xC4,
	NewStruct0 = 0xC5,
	NewStruct = 0xC6,
	NewMap = 0xC8,
	Size = 0xCA,
	HasKey = 0xCB,
	Keys = 0xCC,
	Values = 0xCD,
	PickItem = 0xCE,
	Append = 0xCF,
	SetItem = 0xD0,
	ReverseItems = 0xD1,
	Remove = 0xD2,
	ClearItems = 0xD3,
	PopItem = 0xD4,

	IsNull = 0xD8,
	IsType = 0xD9,
	Convert = 0xDB,

	AbortMsg = 0xE0,
	AssertMsg = 0xE1,
}

struct OperandSize {
	prefix: u8,
	size: u8,
}

lazy_static! {
	static ref OPERAND_SIZES: HashMap<OpCode, OperandSize> = {
		let mut m = HashMap::new();
		m.insert(OpCode::PushInt8, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::PushInt16, OperandSize { prefix: 0, size: 2 });
		m.insert(OpCode::PushInt32, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::PushInt64, OperandSize { prefix: 0, size: 8 });
		m.insert(OpCode::PushInt128, OperandSize { prefix: 0, size: 16 });
		m.insert(OpCode::PushInt256, OperandSize { prefix: 0, size: 32 });
		m.insert(OpCode::PushA, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::PushData1, OperandSize { prefix: 1, size: 0 });
		m.insert(OpCode::PushData2, OperandSize { prefix: 2, size: 0 });
		m.insert(OpCode::PushData4, OperandSize { prefix: 4, size: 0 });
		m.insert(OpCode::Jmp, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::JmpL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::JmpIf, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::JmpIfL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::JmpIfNot, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::JmpIfNotL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::JmpEq, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::JmpEqL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::JmpNe, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::JmpNeL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::JmpGt, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::JmpGtL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::JmpGe, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::JmpGeL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::JmpLt, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::JmpLtL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::JmpLe, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::JmpLeL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::Call, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::CallL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::CallT, OperandSize { prefix: 0, size: 2 });
		m.insert(OpCode::Try, OperandSize { prefix: 0, size: 2 });
		m.insert(OpCode::TryL, OperandSize { prefix: 0, size: 8 });
		m.insert(OpCode::EndTry, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::EndTryL, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::Syscall, OperandSize { prefix: 0, size: 4 });
		m.insert(OpCode::InitSSLot, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::InitSlot, OperandSize { prefix: 0, size: 2 });
		m.insert(OpCode::LdSFLd, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::StSFLd, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::LdLoc, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::StLoc, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::LdArg, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::StArg, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::NewArrayT, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::IsType, OperandSize { prefix: 0, size: 1 });
		m.insert(OpCode::Convert, OperandSize { prefix: 0, size: 1 });

		m
	};
}

impl OpCode {
	pub fn operand_size(&self) -> Result<u8, Error> {
		match OPERAND_SIZES.get(self) {
			Some(size) => Ok(size.size),
			None => Err(VMError::InvalidOpCode(*self)),
		}
	}

	pub fn operand_prefix(&self) -> Result<u8, Error> {
		match OPERAND_SIZES.get(self) {
			Some(size) => Ok(size.prefix),
			None => Err(VMError::InvalidOpCode(*self)),
		}
	}
}

// let opcode_sizes = {
// OpCode::PUSHINT8 => 1,
// OpCode::PUSHINT16 => 2,
// OpCode::PUSHINT32 => 4,
// OpCode::PUSHINT64 => 8,
// OpCode::PUSHINT128 => 16,
// OpCode::PUSHINT256 => 32,
// OpCode::PUSHA => 4,
// OpCode::PUSHDATA1 => 1,
// OpCode::PUSHDATA2 => 2,
// OpCode::PUSHDATA4 => 4,
// OpCode::JMP => 1,
// OpCode::JMP_L => 4,
// OpCode::JMPIF => 1,
// OpCode::JMPIF_L => 4,
// OpCode::JMPIFNOT => 1,
// OpCode::JMPIFNOT_L => 4,
// OpCode::JMPEQ => 1,
// OpCode::JMPEQ_L => 4,
// OpCode::JMPNE => 1,
// OpCode::JMPNE_L => 4,
// OpCode::JMPGT => 1,
// OpCode::JMPGT_L => 4,
// OpCode::JMPGE => 1,
// OpCode::JMPGE_L => 4,
// OpCode::JMPLT => 1,
// OpCode::JMPLT_L => 4,
// OpCode::JMPLE => 1,
// OpCode::JMPLE_L => 4,
// OpCode::CALL => 1,
// OpCode::CALL_L => 4,
// OpCode::CALLT => 2,
// OpCode::TRY => 2,
// OpCode::TRY_L => 8,
// OpCode::ENDTRY => 1,
// OpCode::ENDTRY_L => 4,
// OpCode::XDROP => 1,
// OpCode::PICK => 1,
// OpCode::LDSFLD => 1,
// OpCode::STSFLD => 1,
// OpCode::LDLOC => 1,
// OpCode::STLOC => 1,
// OpCode::LDARG => 1,
// OpCode::STARG => 1,
// OpCode::NEWARRAY_T => 1,
// OpCode::ISTYPE => 1,
// OpCode::CONVERT => 1,
// OpCode::ABORTMSG => 0,
// OpCode::ASSERTMSG => 0,
// };
