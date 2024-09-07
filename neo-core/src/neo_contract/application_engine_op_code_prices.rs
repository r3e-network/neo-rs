// Copyright (C) 2015-2024 The Neo Project.
//
// ApplicationEngine.OpCodePrices.cs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_vm::OpCode;
use std::collections::HashMap;

pub struct ApplicationEngine;

impl ApplicationEngine {
    /// The prices of all the opcodes.
    #[deprecated(note = "You should use OP_CODE_PRICE_TABLE")]
    pub static OP_CODE_PRICES: HashMap<OpCode, i64> = {
        let mut map = HashMap::new();
        map.insert(OpCode::PUSHINT8, 1 << 0);
        map.insert(OpCode::PUSHINT16, 1 << 0);
        map.insert(OpCode::PUSHINT32, 1 << 0);
        map.insert(OpCode::PUSHINT64, 1 << 0);
        map.insert(OpCode::PUSHINT128, 1 << 2);
        map.insert(OpCode::PUSHINT256, 1 << 2);
        map.insert(OpCode::PUSHT, 1 << 0);
        map.insert(OpCode::PUSHF, 1 << 0);
        map.insert(OpCode::PUSHA, 1 << 2);
        map.insert(OpCode::PUSHNULL, 1 << 0);
        map.insert(OpCode::PUSHDATA1, 1 << 3);
        map.insert(OpCode::PUSHDATA2, 1 << 9);
        map.insert(OpCode::PUSHDATA4, 1 << 12);
        map.insert(OpCode::PUSHM1, 1 << 0);
        map.insert(OpCode::PUSH0, 1 << 0);
        map.insert(OpCode::PUSH1, 1 << 0);
        map.insert(OpCode::PUSH2, 1 << 0);
        map.insert(OpCode::PUSH3, 1 << 0);
        map.insert(OpCode::PUSH4, 1 << 0);
        map.insert(OpCode::PUSH5, 1 << 0);
        map.insert(OpCode::PUSH6, 1 << 0);
        map.insert(OpCode::PUSH7, 1 << 0);
        map.insert(OpCode::PUSH8, 1 << 0);
        map.insert(OpCode::PUSH9, 1 << 0);
        map.insert(OpCode::PUSH10, 1 << 0);
        map.insert(OpCode::PUSH11, 1 << 0);
        map.insert(OpCode::PUSH12, 1 << 0);
        map.insert(OpCode::PUSH13, 1 << 0);
        map.insert(OpCode::PUSH14, 1 << 0);
        map.insert(OpCode::PUSH15, 1 << 0);
        map.insert(OpCode::PUSH16, 1 << 0);
        map.insert(OpCode::NOP, 1 << 0);
        map.insert(OpCode::JMP, 1 << 1);
        map.insert(OpCode::JMP_L, 1 << 1);
        map.insert(OpCode::JMPIF, 1 << 1);
        map.insert(OpCode::JMPIF_L, 1 << 1);
        map.insert(OpCode::JMPIFNOT, 1 << 1);
        map.insert(OpCode::JMPIFNOT_L, 1 << 1);
        map.insert(OpCode::JMPEQ, 1 << 1);
        map.insert(OpCode::JMPEQ_L, 1 << 1);
        map.insert(OpCode::JMPNE, 1 << 1);
        map.insert(OpCode::JMPNE_L, 1 << 1);
        map.insert(OpCode::JMPGT, 1 << 1);
        map.insert(OpCode::JMPGT_L, 1 << 1);
        map.insert(OpCode::JMPGE, 1 << 1);
        map.insert(OpCode::JMPGE_L, 1 << 1);
        map.insert(OpCode::JMPLT, 1 << 1);
        map.insert(OpCode::JMPLT_L, 1 << 1);
        map.insert(OpCode::JMPLE, 1 << 1);
        map.insert(OpCode::JMPLE_L, 1 << 1);
        map.insert(OpCode::CALL, 1 << 9);
        map.insert(OpCode::CALL_L, 1 << 9);
        map.insert(OpCode::CALLA, 1 << 9);
        map.insert(OpCode::CALLT, 1 << 15);
        map.insert(OpCode::ABORT, 0);
        map.insert(OpCode::ABORTMSG, 0);
        map.insert(OpCode::ASSERT, 1 << 0);
        map.insert(OpCode::ASSERTMSG, 1 << 0);
        map.insert(OpCode::THROW, 1 << 9);
        map.insert(OpCode::TRY, 1 << 2);
        map.insert(OpCode::TRY_L, 1 << 2);
        map.insert(OpCode::ENDTRY, 1 << 2);
        map.insert(OpCode::ENDTRY_L, 1 << 2);
        map.insert(OpCode::ENDFINALLY, 1 << 2);
        map.insert(OpCode::RET, 0);
        map.insert(OpCode::SYSCALL, 0);
        map.insert(OpCode::DEPTH, 1 << 1);
        map.insert(OpCode::DROP, 1 << 1);
        map.insert(OpCode::NIP, 1 << 1);
        map.insert(OpCode::XDROP, 1 << 4);
        map.insert(OpCode::CLEAR, 1 << 4);
        map.insert(OpCode::DUP, 1 << 1);
        map.insert(OpCode::OVER, 1 << 1);
        map.insert(OpCode::PICK, 1 << 1);
        map.insert(OpCode::TUCK, 1 << 1);
        map.insert(OpCode::SWAP, 1 << 1);
        map.insert(OpCode::ROT, 1 << 1);
        map.insert(OpCode::ROLL, 1 << 4);
        map.insert(OpCode::REVERSE3, 1 << 1);
        map.insert(OpCode::REVERSE4, 1 << 1);
        map.insert(OpCode::REVERSEN, 1 << 4);
        map.insert(OpCode::INITSSLOT, 1 << 4);
        map.insert(OpCode::INITSLOT, 1 << 6);
        map.insert(OpCode::LDSFLD0, 1 << 1);
        map.insert(OpCode::LDSFLD1, 1 << 1);
        map.insert(OpCode::LDSFLD2, 1 << 1);
        map.insert(OpCode::LDSFLD3, 1 << 1);
        map.insert(OpCode::LDSFLD4, 1 << 1);
        map.insert(OpCode::LDSFLD5, 1 << 1);
        map.insert(OpCode::LDSFLD6, 1 << 1);
        map.insert(OpCode::LDSFLD, 1 << 1);
        map.insert(OpCode::STSFLD0, 1 << 1);
        map.insert(OpCode::STSFLD1, 1 << 1);
        map.insert(OpCode::STSFLD2, 1 << 1);
        map.insert(OpCode::STSFLD3, 1 << 1);
        map.insert(OpCode::STSFLD4, 1 << 1);
        map.insert(OpCode::STSFLD5, 1 << 1);
        map.insert(OpCode::STSFLD6, 1 << 1);
        map.insert(OpCode::STSFLD, 1 << 1);
        map.insert(OpCode::LDLOC0, 1 << 1);
        map.insert(OpCode::LDLOC1, 1 << 1);
        map.insert(OpCode::LDLOC2, 1 << 1);
        map.insert(OpCode::LDLOC3, 1 << 1);
        map.insert(OpCode::LDLOC4, 1 << 1);
        map.insert(OpCode::LDLOC5, 1 << 1);
        map.insert(OpCode::LDLOC6, 1 << 1);
        map.insert(OpCode::LDLOC, 1 << 1);
        map.insert(OpCode::STLOC0, 1 << 1);
        map.insert(OpCode::STLOC1, 1 << 1);
        map.insert(OpCode::STLOC2, 1 << 1);
        map.insert(OpCode::STLOC3, 1 << 1);
        map.insert(OpCode::STLOC4, 1 << 1);
        map.insert(OpCode::STLOC5, 1 << 1);
        map.insert(OpCode::STLOC6, 1 << 1);
        map.insert(OpCode::STLOC, 1 << 1);
        map.insert(OpCode::LDARG0, 1 << 1);
        map.insert(OpCode::LDARG1, 1 << 1);
        map.insert(OpCode::LDARG2, 1 << 1);
        map.insert(OpCode::LDARG3, 1 << 1);
        map.insert(OpCode::LDARG4, 1 << 1);
        map.insert(OpCode::LDARG5, 1 << 1);
        map.insert(OpCode::LDARG6, 1 << 1);
        map.insert(OpCode::LDARG, 1 << 1);
        map.insert(OpCode::STARG0, 1 << 1);
        map.insert(OpCode::STARG1, 1 << 1);
        map.insert(OpCode::STARG2, 1 << 1);
        map.insert(OpCode::STARG3, 1 << 1);
        map.insert(OpCode::STARG4, 1 << 1);
        map.insert(OpCode::STARG5, 1 << 1);
        map.insert(OpCode::STARG6, 1 << 1);
        map.insert(OpCode::STARG, 1 << 1);
        map.insert(OpCode::NEWBUFFER, 1 << 8);
        map.insert(OpCode::MEMCPY, 1 << 11);
        map.insert(OpCode::CAT, 1 << 11);
        map.insert(OpCode::SUBSTR, 1 << 11);
        map.insert(OpCode::LEFT, 1 << 11);
        map.insert(OpCode::RIGHT, 1 << 11);
        map.insert(OpCode::INVERT, 1 << 2);
        map.insert(OpCode::AND, 1 << 3);
        map.insert(OpCode::OR, 1 << 3);
        map.insert(OpCode::XOR, 1 << 3);
        map.insert(OpCode::EQUAL, 1 << 5);
        map.insert(OpCode::NOTEQUAL, 1 << 5);
        map.insert(OpCode::SIGN, 1 << 2);
        map.insert(OpCode::ABS, 1 << 2);
        map.insert(OpCode::NEGATE, 1 << 2);
        map.insert(OpCode::INC, 1 << 2);
        map.insert(OpCode::DEC, 1 << 2);
        map.insert(OpCode::ADD, 1 << 3);
        map.insert(OpCode::SUB, 1 << 3);
        map.insert(OpCode::MUL, 1 << 3);
        map.insert(OpCode::DIV, 1 << 3);
        map.insert(OpCode::MOD, 1 << 3);
        map.insert(OpCode::POW, 1 << 6);
        map.insert(OpCode::SQRT, 1 << 6);
        map.insert(OpCode::MODMUL, 1 << 5);
        map.insert(OpCode::MODPOW, 1 << 11);
        map.insert(OpCode::SHL, 1 << 3);
        map.insert(OpCode::SHR, 1 << 3);
        map.insert(OpCode::NOT, 1 << 2);
        map.insert(OpCode::BOOLAND, 1 << 3);
        map.insert(OpCode::BOOLOR, 1 << 3);
        map.insert(OpCode::NZ, 1 << 2);
        map.insert(OpCode::NUMEQUAL, 1 << 3);
        map.insert(OpCode::NUMNOTEQUAL, 1 << 3);
        map.insert(OpCode::LT, 1 << 3);
        map.insert(OpCode::LE, 1 << 3);
        map.insert(OpCode::GT, 1 << 3);
        map.insert(OpCode::GE, 1 << 3);
        map.insert(OpCode::MIN, 1 << 3);
        map.insert(OpCode::MAX, 1 << 3);
        map.insert(OpCode::WITHIN, 1 << 3);
        map.insert(OpCode::PACKMAP, 1 << 11);
        map.insert(OpCode::PACKSTRUCT, 1 << 11);
        map.insert(OpCode::PACK, 1 << 11);
        map.insert(OpCode::UNPACK, 1 << 11);
        map.insert(OpCode::NEWARRAY0, 1 << 4);
        map.insert(OpCode::NEWARRAY, 1 << 9);
        map.insert(OpCode::NEWARRAY_T, 1 << 9);
        map.insert(OpCode::NEWSTRUCT0, 1 << 4);
        map.insert(OpCode::NEWSTRUCT, 1 << 9);
        map.insert(OpCode::NEWMAP, 1 << 3);
        map.insert(OpCode::SIZE, 1 << 2);
        map.insert(OpCode::HASKEY, 1 << 6);
        map.insert(OpCode::KEYS, 1 << 4);
        map.insert(OpCode::VALUES, 1 << 13);
        map.insert(OpCode::PICKITEM, 1 << 6);
        map.insert(OpCode::APPEND, 1 << 13);
        map.insert(OpCode::SETITEM, 1 << 13);
        map.insert(OpCode::REVERSEITEMS, 1 << 13);
        map.insert(OpCode::REMOVE, 1 << 4);
        map.insert(OpCode::CLEARITEMS, 1 << 4);
        map.insert(OpCode::POPITEM, 1 << 4);
        map.insert(OpCode::ISNULL, 1 << 1);
        map.insert(OpCode::ISTYPE, 1 << 1);
        map.insert(OpCode::CONVERT, 1 << 13);
        map
    };

    /// The prices of all the opcodes.
    /// In the unit of datoshi, 1 datoshi = 1e-8 GAS
    pub static OP_CODE_PRICE_TABLE: [i64; 256] = {
        let mut table = [0; 256];
        for (op_code, price) in Self::OP_CODE_PRICES.iter() {
            table[*op_code as usize] = *price;
        }
        table
    };
}
