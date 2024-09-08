use crate::op_code::OpCode;
use num_bigint::BigInt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash,Serialize,Deserialize)]
pub struct ScriptBuilder {
	output: Vec<u8>,
}

impl ScriptBuilder {
	pub fn new() -> Self {
		Self { output: Vec::new() }
	}

	pub fn len(&self) -> usize {
		self.output.len()
	}

	pub fn push_int(&mut self, value: i64) {
		if value >= 0 && value <= 16 {
			let opcode = OpCode::Push0 + (value as u8);
			self.emit(opcode, Vec::new());
			return
		}

		let is_negative = value < 0;
		let mut abs_value = if is_negative { -value } else { value };

		// let mut buffer = [0u8; 32];
		let bytes_written = abs_value.to_le_bytes();
		// buffer[..bytes_written.len()].copy_from_slice(&bytes_written);

		let written_len = bytes_written.len();
		let (opcode, pad_len) = match bytes_written.len() {
			1 => (OpCode::PushInt8, 1),
			2 => (OpCode::PushInt16, 2),
			bytes if bytes <= 4 => (OpCode::PushInt32, 4),
			bytes if bytes <= 8 => (OpCode::PushInt64, 8),
			bytes if bytes <= 16 => (OpCode::PushInt128, 16),
			_ => (OpCode::PushInt256, 32),
		};

		let sign_byte = if is_negative { 0xFF } else { 0x00 };
		let mut padded = vec![sign_byte; pad_len];
		padded[pad_len - written_len..].copy_from_slice(&bytes_written);

		self.raw(opcode, Vec::from(padded));
	}

	pub fn push_bool(&mut self, value: bool) {
		let opcode = if value { OpCode::PushTrue } else { OpCode::PushFalse };

		self.output.push(opcode as u8);
	}

	pub fn push_bytes(&mut self, data: Vec<u8>) {
		match data.len() {
			len if len < 0x100 => {
				self.output.push(OpCode::PushData1 as u8);
				self.output.push(len as u8);
				self.output.extend_from_slice(&data);
			},
			len if len < 0x10000 => {
				self.output.push(OpCode::PushData2 as u8);
				self.output.extend_from_slice(&(len as u16).to_le_bytes());
				self.output.extend_from_slice(&data);
			},
			len => {
				self.output.push(OpCode::PushData4 as u8);
				self.output.extend_from_slice(&(len as u32).to_le_bytes());
				self.output.extend_from_slice(&data);
			},
		}
	}

	// Other push methods
	pub fn push_string(&mut self, data: &str) {
		let bytes = data.as_bytes().to_vec();
		self.push_bytes(bytes);
	}

	pub fn push_call(&mut self, offset: i32) {
		if offset >= i8::MIN as i32 && offset <= i8::MAX as i32 {
			let opcode = OpCode::Call;
			let operand = vec![offset as u8];
			self.raw(opcode, operand);
		} else {
			let opcode = OpCode::CallL;
			let operand = offset.to_le_bytes().to_vec();
			self.raw(opcode, operand);
		}
	}

	pub fn push_jump(&mut self, opcode: OpCode, offset: i32) {
		if (opcode as i32) >= OpCode::Jmp as i32 && (opcode as i32) <= OpCode::JmpLeL as i32 {
			let operand = if offset >= i8::MIN as i32 && offset <= i8::MAX as i32 {
				vec![offset as u8]
			} else {
				offset.to_le_bytes().to_vec()
			};

			self.raw(opcode, operand);
		} else {
			panic!("Invalid opcode for jump instruction");
		}
	}
	pub fn push_syscall(&mut self, api: u32) {
		let opcode = OpCode::Syscall;
		let operand = api.to_le_bytes().to_vec();
		self.raw(opcode, operand);
	}

	fn raw(&mut self, opcode: OpCode, operand: Vec<u8>) {
		self.output.push(opcode as u8);
		self.output.extend_from_slice(&operand);
	}

	pub fn push_null(&mut self) {
		let opcode = OpCode::PushNull;
		self.raw(opcode, Vec::new());
	}

	pub fn push_zero(&mut self) {
		let opcode = OpCode::Push0;
		self.raw(opcode, Vec::new());
	}

	pub fn push_data(&mut self, data: Vec<u8>) {
		self.push_bytes(data);
	}

	pub fn push_data_byte(&mut self, byte: u8) {
		let mut bytes = Vec::from([byte]);
		self.push_bytes(bytes);
	}

	// pub fn push_bytes1(&mut self, data: Vec<u8>) {
	//     if data.len() == 1 {
	//         let opcode = OpCode::PUSHBYTES1;
	//         let mut operand = vec![data[0]];
	//         self.raw(opcode, operand);
	//     } else {
	//         panic!("PUSHBYES1 operand must be 1 byte");
	//     }
	// }
	//
	// pub fn push_bytes75(&mut self, data: Vec<u8>) {
	//     if data.len() <= 75 {
	//         let opcode = OpCode::PUSHBYTES75 + data.len() as u8;
	//         self.raw(opcode, data);
	//     } else {
	//         panic!("PUSHBYTES75 operand cannot exceed 75 bytes");
	//     }
	// }

	pub fn push_bytes_var(&mut self, data: Vec<u8>) {
		match data.len() {
			0...75 => self.push_bytes75(data),
			76...0xff => {
				let opcode = OpCode::PushData1;
				let len = data.len() as u8;
				let mut operand = vec![len];
				operand.extend_from_slice(&data);
				self.raw(opcode, operand);
			},
			0x100...0xffff => {
				let opcode = OpCode::PushData2;
				let len = data.len() as u16;
				let mut operand = len.to_le_bytes().to_vec();
				operand.extend_from_slice(&data);
				self.raw(opcode, operand);
			},
			_ => {
				let opcode = OpCode::PushData4;
				let len = data.len() as u32;
				let mut operand = len.to_le_bytes().to_vec();
				operand.extend_from_slice(&data);
				self.raw(opcode, operand);
			},
		}
	}

	pub fn to_bytes(self) -> Vec<u8> {
		self.output
	}
}
