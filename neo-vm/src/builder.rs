// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use bytes::{BufMut, BytesMut};

use neo_base::math::I256;

use crate::{OpCode, OpCode::*};


#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u8)]
pub enum Jump {
    Jmp = 0x22,
    JmpIf = 0x24,
    JmpIfNot = 0x26,
    JmpEq = 0x28,
    JmpNe = 0x2A,
    JmpGt = 0x2C,
    JmpGe = 0x2E,
    JmpLt = 0x30,
    JmpLe = 0x32,
}


pub struct ScriptBuilder {
    buf: BytesMut,
}

impl ScriptBuilder {
    #[inline]
    pub fn new() -> Self { Self { buf: BytesMut::new() } }

    #[inline]
    pub fn emit(&mut self, opcode: OpCode) {
        self.buf.put_u8(opcode.as_u8());
    }

    pub fn emit_jmp(&mut self, jump: Jump, offset: i32) {
        if offset >= i8::MIN as i32 && offset <= i8::MAX as i32 {
            self.buf.put_u8(jump as u8);
            self.buf.put_i8(offset as i8)
        } else {
            self.buf.put_u8(jump as u8 + 1);
            self.buf.put_i32_le(offset);
        }
    }

    pub fn emit_try(&mut self, catch: i32, finally: i32) {
        if (catch >= i8::MIN as i32 && catch <= i8::MAX as i32)
            && (finally >= i8::MIN as i32 && finally <= i8::MAX as i32) {
            self.buf.put_u8(Try.as_u8());
            self.buf.put_i8(catch as i8);
            self.buf.put_i8(finally as i8);
        } else {
            self.buf.put_u8(TryL.as_u8());
            self.buf.put_i32_le(catch);
            self.buf.put_i32_le(finally);
        }
    }

    pub fn emit_try_end(&mut self, try_end: i32) {
        if try_end >= i8::MIN as i32 && try_end <= i8::MAX as i32 {
            self.buf.put_u8(EndTry.as_u8());
            self.buf.put_i8(try_end as i8);
        } else {
            self.buf.put_u8(EndTryL.as_u8());
            self.buf.put_i32_le(try_end);
        }
    }

    pub fn emit_call(&mut self, offset: i32) {
        if offset >= i8::MIN as i32 && offset <= i8::MAX as i32 {
            self.buf.put_u8(Call.as_u8());
            self.buf.put_i8(offset as i8);
        } else {
            self.buf.put_u8(CallL.as_u8());
            self.buf.put_i32_le(offset);
        }
    }

    #[inline]
    pub fn emit_syscall(mut self, syscall: u32) {
        self.buf.put_u8(Syscall.as_u8());
        self.buf.put_u32_le(syscall);
    }

    #[inline]
    pub fn emit_push_bool(&mut self, v: bool) {
        self.buf.put_u8(if v { PushTrue.as_u8() } else { PushFalse.as_u8() });
    }

    pub fn emit_push_n(&mut self, n: i64) {
        if n >= -1 && n <= 16 {
            self.buf.put_u8(Push0.as_u8() + n as u8);
        } else if n >= i8::MIN as i64 && n <= i8::MAX as i64 {
            self.buf.put_u8(PushInt8.as_u8());
            self.buf.put_i8(n as i8);
        } else if n >= i16::MIN as i64 && n <= i16::MAX as i64 {
            self.buf.put_u8(PushInt16.as_u8());
            self.buf.put_i16_le(n as i16);
        } else if n >= i32::MIN as i64 && n <= i32::MAX as i64 {
            self.buf.put_u8(PushInt32.as_u8());
            self.buf.put_i32_le(n as i32);
        } else {
            self.buf.put_u8(PushInt64.as_u8());
            self.buf.put_i64_le(n);
        }
    }

    #[inline]
    pub fn emit_push_i128(&mut self, n: i128) {
        self.buf.put_u8(PushInt128.as_u8());
        self.buf.put_i128_le(n);
    }

    #[inline]
    pub fn emit_push_i256(&mut self, n: I256) {
        self.buf.put_u8(PushInt256.as_u8());
        self.buf.put_slice(&n.to_le_bytes())
    }

    pub fn emit_push_data(&mut self, data: &[u8]) -> bool {
        if data.len() < 0x100 {
            self.buf.put_u8(PushData1.as_u8());
        } else if data.len() < 0x10000 {
            self.buf.put_u8(PushData2.as_u8());
        } else if data.len() < 0x100000000 {
            self.buf.put_u8(PushData4.as_u8());
        } else {
            return false;
        }

        self.buf.put_slice(data);
        true
    }

    #[inline]
    pub fn emit_with_operand(&mut self, opcode: OpCode, first: u8) {
        self.buf.put_u8(opcode.as_u8());
        self.buf.put_u8(first);
    }

    #[inline]
    pub fn emit_static_slot(&mut self, n: u8) {
        self.buf.put_u8(InitSSLot.as_u8());
        self.buf.put_u8(n);
    }

    #[inline]
    pub fn emit_slot(&mut self, locals: u8, arguments: u8) {
        self.buf.put_u8(InitSSLot.as_u8());
        self.buf.put_u8(locals);
        self.buf.put_u8(arguments);
    }
}
