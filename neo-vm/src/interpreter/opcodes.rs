// Many constants are used via range patterns (e.g., LDLOC0..=LDLOC6) and
// appear "unused" to the compiler. Suppress the warning at module scope.
#![allow(dead_code)]

use crate::OpCode;

macro_rules! opcode_alias {
    ($name:ident) => {
        pub(crate) const $name: u8 = OpCode::$name.byte();
    };
}

// Push
opcode_alias!(PUSHINT8);
opcode_alias!(PUSHINT16);
opcode_alias!(PUSHINT32);
opcode_alias!(PUSHINT64);
opcode_alias!(PUSHINT128);
opcode_alias!(PUSHINT256);
opcode_alias!(PUSHT);
opcode_alias!(PUSHF);
opcode_alias!(PUSHA);
opcode_alias!(PUSHNULL);
opcode_alias!(PUSHDATA1);
opcode_alias!(PUSHDATA2);
opcode_alias!(PUSHDATA4);
opcode_alias!(PUSHM1);
opcode_alias!(PUSH0);
opcode_alias!(PUSH1);
opcode_alias!(PUSH2);
opcode_alias!(PUSH3);
opcode_alias!(PUSH4);
opcode_alias!(PUSH5);
opcode_alias!(PUSH6);
opcode_alias!(PUSH7);
opcode_alias!(PUSH8);
opcode_alias!(PUSH9);
opcode_alias!(PUSH10);
opcode_alias!(PUSH11);
opcode_alias!(PUSH12);
opcode_alias!(PUSH13);
opcode_alias!(PUSH14);
opcode_alias!(PUSH15);
opcode_alias!(PUSH16);

// Flow control
opcode_alias!(NOP);
opcode_alias!(JMP);
opcode_alias!(JMP_L);
opcode_alias!(JMPIF);
opcode_alias!(JMPIF_L);
opcode_alias!(JMPIFNOT);
opcode_alias!(JMPIFNOT_L);
opcode_alias!(JMPEQ);
opcode_alias!(JMPEQ_L);
opcode_alias!(JMPNE);
opcode_alias!(JMPNE_L);
opcode_alias!(JMPGT);
opcode_alias!(JMPGT_L);
opcode_alias!(JMPGE);
opcode_alias!(JMPGE_L);
opcode_alias!(JMPLT);
opcode_alias!(JMPLT_L);
opcode_alias!(JMPLE);
opcode_alias!(JMPLE_L);
opcode_alias!(CALL);
opcode_alias!(CALL_L);
opcode_alias!(CALLA);
opcode_alias!(CALLT);
opcode_alias!(ABORT);
opcode_alias!(ASSERT);
opcode_alias!(THROW);
opcode_alias!(TRY);
opcode_alias!(TRY_L);
opcode_alias!(ENDTRY);
opcode_alias!(ENDTRY_L);
opcode_alias!(ENDFINALLY);
opcode_alias!(RET);
opcode_alias!(SYSCALL);
opcode_alias!(ABORTMSG);
opcode_alias!(ASSERTMSG);

// Stack
opcode_alias!(DEPTH);
opcode_alias!(DROP);
opcode_alias!(NIP);
opcode_alias!(XDROP);
opcode_alias!(CLEAR);
opcode_alias!(DUP);
opcode_alias!(OVER);
opcode_alias!(PICK);
opcode_alias!(TUCK);
opcode_alias!(SWAP);
opcode_alias!(ROT);
opcode_alias!(ROLL);
opcode_alias!(REVERSE3);
opcode_alias!(REVERSE4);
opcode_alias!(REVERSEN);

// Slot
opcode_alias!(INITSSLOT);
opcode_alias!(INITSLOT);
opcode_alias!(LDSFLD0);
opcode_alias!(LDSFLD1);
opcode_alias!(LDSFLD2);
opcode_alias!(LDSFLD3);
opcode_alias!(LDSFLD4);
opcode_alias!(LDSFLD5);
opcode_alias!(LDSFLD6);
opcode_alias!(LDSFLD);
opcode_alias!(STSFLD0);
opcode_alias!(STSFLD1);
opcode_alias!(STSFLD2);
opcode_alias!(STSFLD3);
opcode_alias!(STSFLD4);
opcode_alias!(STSFLD5);
opcode_alias!(STSFLD6);
opcode_alias!(STSFLD);
opcode_alias!(LDLOC0);
opcode_alias!(LDLOC1);
opcode_alias!(LDLOC2);
opcode_alias!(LDLOC3);
opcode_alias!(LDLOC4);
opcode_alias!(LDLOC5);
opcode_alias!(LDLOC6);
opcode_alias!(LDLOC);
opcode_alias!(STLOC0);
opcode_alias!(STLOC1);
opcode_alias!(STLOC2);
opcode_alias!(STLOC3);
opcode_alias!(STLOC4);
opcode_alias!(STLOC5);
opcode_alias!(STLOC6);
opcode_alias!(STLOC);
opcode_alias!(LDARG0);
opcode_alias!(LDARG1);
opcode_alias!(LDARG2);
opcode_alias!(LDARG3);
opcode_alias!(LDARG4);
opcode_alias!(LDARG5);
opcode_alias!(LDARG6);
opcode_alias!(LDARG);
opcode_alias!(STARG0);
opcode_alias!(STARG1);
opcode_alias!(STARG2);
opcode_alias!(STARG3);
opcode_alias!(STARG4);
opcode_alias!(STARG5);
opcode_alias!(STARG6);
opcode_alias!(STARG);

// Splice
opcode_alias!(NEWBUFFER);
opcode_alias!(MEMCPY);
opcode_alias!(CAT);
opcode_alias!(SUBSTR);
opcode_alias!(LEFT);
opcode_alias!(RIGHT);

// Bitwise logic
opcode_alias!(INVERT);
opcode_alias!(AND);
opcode_alias!(OR);
opcode_alias!(XOR);
opcode_alias!(EQUAL);
opcode_alias!(NOTEQUAL);

// Arithmetic
opcode_alias!(SIGN);
opcode_alias!(ABS);
opcode_alias!(NEGATE);
opcode_alias!(INC);
opcode_alias!(DEC);
opcode_alias!(ADD);
opcode_alias!(SUB);
opcode_alias!(MUL);
opcode_alias!(DIV);
opcode_alias!(MOD);
opcode_alias!(POW);
opcode_alias!(SQRT);
opcode_alias!(MODMUL);
opcode_alias!(MODPOW);
opcode_alias!(SHL);
opcode_alias!(SHR);
opcode_alias!(NOT);
opcode_alias!(BOOLAND);
opcode_alias!(BOOLOR);
opcode_alias!(NZ);
opcode_alias!(NUMEQUAL);
opcode_alias!(NUMNOTEQUAL);
opcode_alias!(LT);
opcode_alias!(LE);
opcode_alias!(GT);
opcode_alias!(GE);
opcode_alias!(MIN);
opcode_alias!(MAX);
opcode_alias!(WITHIN);

// Compound types
opcode_alias!(PACKMAP);
opcode_alias!(PACKSTRUCT);
opcode_alias!(PACK);
opcode_alias!(UNPACK);
opcode_alias!(NEWARRAY0);
opcode_alias!(NEWARRAY);
opcode_alias!(NEWARRAY_T);
opcode_alias!(NEWSTRUCT0);
opcode_alias!(NEWSTRUCT);
opcode_alias!(NEWMAP);
opcode_alias!(SIZE);
opcode_alias!(HASKEY);
opcode_alias!(KEYS);
opcode_alias!(VALUES);
opcode_alias!(PICKITEM);
opcode_alias!(APPEND);
opcode_alias!(SETITEM);
opcode_alias!(REVERSEITEMS);
opcode_alias!(REMOVE);
opcode_alias!(CLEARITEMS);
opcode_alias!(POPITEM);

// Types
opcode_alias!(ISNULL);
opcode_alias!(ISTYPE);
opcode_alias!(CONVERT);
