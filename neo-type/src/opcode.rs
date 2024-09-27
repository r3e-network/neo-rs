// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use strum_macros::EnumIter;
use crate::OpCode::*;

#[derive(Debug, Copy, Clone, Eq, PartialEq, EnumIter)]
#[repr(u8)]
pub enum OpCode {
    PushInt8   = 0x00,
    PushInt16  = 0x01,
    PushInt32  = 0x02,
    PushInt64  = 0x03,
    PushInt128 = 0x04,
    PushInt256 = 0x05,

    PushTrue   = 0x08,
    PushFalse  = 0x09,
    PushA      = 0x0A,
    PushNull   = 0x0B,
    PushData1  = 0x0C,
    PushData2  = 0x0D,
    PushData4  = 0x0E,

    PushM1     = 0x0F,
    Push0      = 0x10,
    Push1      = 0x11,
    Push2      = 0x12,
    Push3      = 0x13,
    Push4      = 0x14,
    Push5      = 0x15,
    Push6      = 0x16,
    Push7      = 0x17,
    Push8      = 0x18,
    Push9      = 0x19,
    Push10     = 0x1A,
    Push11     = 0x1B,
    Push12     = 0x1C,
    Push13     = 0x1D,
    Push14     = 0x1E,
    Push15     = 0x1F,
    Push16     = 0x20,

    Nop        = 0x21,
    Jmp        = 0x22,
    JmpL       = 0x23,
    JmpIf      = 0x24,
    JmpIfL     = 0x25,
    JmpIfNot   = 0x26,
    JmpIfNotL  = 0x27,
    JmpEq      = 0x28,
    JmpEqL     = 0x29,
    JmpNe      = 0x2A,
    JmpNeL     = 0x2B,
    JmpGt      = 0x2C,
    JmpGtL     = 0x2D,
    JmpGe      = 0x2E,
    JmpGeL     = 0x2F,
    JmpLt      = 0x30,
    JmpLtL     = 0x31,
    JmpLe      = 0x32,
    JmpLeL     = 0x33,

    Call       = 0x34,
    CallL      = 0x35,
    CallA      = 0x36,
    CallT      = 0x37,
    Abort      = 0x38,
    Assert     = 0x39,

    Throw      = 0x3A,
    Try        = 0x3B,
    TryL       = 0x3C,
    EndTry     = 0x3D,
    EndTryL    = 0x3E,
    EndFinally = 0x3F,

    Return     = 0x40,
    Syscall    = 0x41,

    Depth      = 0x43,
    Drop       = 0x45,
    Nip        = 0x46,
    Xdrop      = 0x48,
    Clear      = 0x49,
    Dup        = 0x4A,
    Over       = 0x4B,
    Pick       = 0x4D,
    Tuck       = 0x4E,

    Swap       = 0x50,
    Rotate     = 0x51,
    Roll       = 0x52,

    Reverse3   = 0x53,
    Reverse4   = 0x54,
    ReverseN   = 0x55,
    InitSSLot  = 0x56,
    InitSlot   = 0x57,

    LdSFLd0    = 0x58,
    LdSFLd1    = 0x59,
    LdSFLd2    = 0x5A,
    LdSFLd3    = 0x5B,
    LdSFLd4    = 0x5C,
    LdSFLd5    = 0x5D,
    LdSFLd6    = 0x5E,
    LdSFLd     = 0x5F,

    StSFLd0    = 0x60,
    StSFLd1    = 0x61,
    StSFLd2    = 0x62,
    StSFLd3    = 0x63,
    StSFLd4    = 0x64,
    StSFLd5    = 0x65,
    StSFLd6    = 0x66,
    StSFLd     = 0x67,

    LdLoc0     = 0x68,
    LdLoc1     = 0x69,
    LdLoc2     = 0x6A,
    LdLoc3     = 0x6B,
    LdLoc4     = 0x6C,
    LdLoc5     = 0x6D,
    LdLoc6     = 0x6E,
    LdLoc      = 0x6F,

    StLoc0     = 0x70,
    StLoc1     = 0x71,
    StLoc2     = 0x72,
    StLoc3     = 0x73,
    StLoc4     = 0x74,
    StLoc5     = 0x75,
    StLoc6     = 0x76,
    StLoc      = 0x77,

    LdArg0     = 0x78,
    LdArg1     = 0x79,
    LdArg2     = 0x7A,
    LdArg3     = 0x7B,
    LdArg4     = 0x7C,
    LdArg5     = 0x7D,
    LdArg6     = 0x7E,
    LdArg      = 0x7F,

    StArg0     = 0x80,
    StArg1     = 0x81,
    StArg2     = 0x82,
    StArg3     = 0x83,
    StArg4     = 0x84,
    StArg5     = 0x85,
    StArg6     = 0x86,
    StArg      = 0x87,

    NewBuffer  = 0x88,
    MemCpy     = 0x89,
    Cat        = 0x8B,
    SubStr     = 0x8C,
    Left       = 0x8D,
    Right      = 0x8E,

    Invert     = 0x90,
    And        = 0x91,
    Or         = 0x92,
    Xor        = 0x93,
    Equal      = 0x97,
    NotEqual   = 0x98,
    Sign       = 0x99,
    Abs        = 0x9A,
    Negate     = 0x9B,

    Inc        = 0x9C,
    Dec        = 0x9D,
    Add        = 0x9E,
    Sub        = 0x9F,
    Mul        = 0xA0,
    Div        = 0xA1,
    Mod        = 0xA2,
    Pow        = 0xA3,
    Sqrt       = 0xA4,
    ModMul     = 0xA5,
    ModPow     = 0xA6,

    Shl        = 0xA8,
    Shr        = 0xA9,
    Not        = 0xAA,
    BoolAnd    = 0xAB,
    BoolOr     = 0xAC,

    Nz         = 0xB1,
    NumEqual   = 0xB3,
    NumNotEqual = 0xB4,
    Lt         = 0xB5,
    Le         = 0xB6,
    Gt         = 0xB7,
    Ge         = 0xB8,
    Min        = 0xB9,
    Max        = 0xBA,
    Within     = 0xBB,

    PackMap    = 0xBE,
    PackStruct = 0xBF,
    Pack       = 0xC0,
    Unpack     = 0xC1,

    NewArray0  = 0xC2,
    NewArray   = 0xC3,
    NewArrayT  = 0xC4,
    NewStruct0 = 0xC5,
    NewStruct  = 0xC6,
    NewMap     = 0xC8,

    Size       = 0xCA,
    HasKey     = 0xCB,
    Keys       = 0xCC,
    Values     = 0xCD,

    PickItem   = 0xCE,
    Append     = 0xCF,
    SetItem    = 0xD0,
    ReverseItems = 0xD1,
    Remove     = 0xD2,
    ClearItems = 0xD3,
    PopItem    = 0xD4,

    IsNull     = 0xD8,
    IsType     = 0xD9,
    Convert    = 0xDB,

    AbortMsg   = 0xE0,
    AssertMsg  = 0xE1,
}

impl OpCode {
    #[inline]
    pub const fn as_u8(&self) -> u8 { *self as u8 }

    #[inline]
    pub const fn from_u8(code: u8) -> Option<OpCode> { OP_CODES[code as usize] }

    #[inline]
    pub const fn is_valid(code: u8) -> bool { OP_CODES[code as usize].is_some() }
}

pub const OP_CODES: [Option<OpCode>; 256] = [
    Some(PushInt8),     // 0x0, 0
    Some(PushInt16),    // 0x1, 1
    Some(PushInt32),    // 0x2, 2
    Some(PushInt64),    // 0x3, 3
    Some(PushInt128),   // 0x4, 4
    Some(PushInt256),   // 0x5, 5
    None,               // Reserved
    None,               // Reserved
    Some(PushTrue),     // 0x8, 8
    Some(PushFalse),    // 0x9, 9
    Some(PushA),        // 0xA, 10
    Some(PushNull),     // 0xB, 11
    Some(PushData1),    // 0xC, 12
    Some(PushData2),    // 0xD, 13
    Some(PushData4),    // 0xE, 14
    Some(PushM1),       // 0xF, 15
    Some(Push0),        // 0x10, 16
    Some(Push1),        // 0x11, 17
    Some(Push2),        // 0x12, 18
    Some(Push3),        // 0x13, 19
    Some(Push4),        // 0x14, 20
    Some(Push5),        // 0x15, 21
    Some(Push6),        // 0x16, 22
    Some(Push7),        // 0x17, 23
    Some(Push8),        // 0x18, 24
    Some(Push9),        // 0x19, 25
    Some(Push10),       // 0x1A, 26
    Some(Push11),       // 0x1B, 27
    Some(Push12),       // 0x1C, 28
    Some(Push13),       // 0x1D, 29
    Some(Push14),       // 0x1E, 30
    Some(Push15),       // 0x1F, 31
    Some(Push16),       // 0x20, 32
    Some(Nop),          // 0x21, 33
    Some(Jmp),          // 0x22, 34
    Some(JmpL),         // 0x23, 35
    Some(JmpIf),        // 0x24, 36
    Some(JmpIfL),       // 0x25, 37
    Some(JmpIfNot),     // 0x26, 38
    Some(JmpIfNotL),    // 0x27, 39
    Some(JmpEq),        // 0x28, 40
    Some(JmpEqL),       // 0x29, 41
    Some(JmpNe),        // 0x2A, 42
    Some(JmpNeL),       // 0x2B, 43
    Some(JmpGt),        // 0x2C, 44
    Some(JmpGtL),       // 0x2D, 45
    Some(JmpGe),        // 0x2E, 46
    Some(JmpGeL),       // 0x2F, 47
    Some(JmpLt),        // 0x30, 48
    Some(JmpLtL),       // 0x31, 49
    Some(JmpLe),        // 0x32, 50
    Some(JmpLeL),       // 0x33, 51
    Some(Call),         // 0x34, 52
    Some(CallL),        // 0x35, 53
    Some(CallA),        // 0x36, 54
    Some(CallT),        // 0x37, 55
    Some(Abort),        // 0x38, 56
    Some(Assert),       // 0x39, 57
    Some(Throw),        // 0x3A, 58
    Some(Try),          // 0x3B, 59
    Some(TryL),         // 0x3C, 60
    Some(EndTry),       // 0x3D, 61
    Some(EndTryL),      // 0x3E, 62
    Some(EndFinally),   // 0x3F, 63
    Some(Return),       // 0x40, 64
    Some(Syscall),      // 0x41, 65
    None,               // Reserved
    Some(Depth),        // 0x43, 67
    None,               // Reserved
    Some(Drop),         // 0x45, 69
    Some(Nip),          // 0x46, 70
    None,               // Reserved
    Some(Xdrop),        // 0x48, 72
    Some(Clear),        // 0x49, 73
    Some(Dup),          // 0x4A, 74
    Some(Over),         // 0x4B, 75
    None,               // Reserved
    Some(Pick),         // 0x4D, 77
    Some(Tuck),         // 0x4E, 78
    None,               // Reserved
    Some(Swap),         // 0x50, 80
    Some(Rotate),       // 0x51, 81
    Some(Roll),         // 0x52, 82
    Some(Reverse3),     // 0x53, 83
    Some(Reverse4),     // 0x54, 84
    Some(ReverseN),     // 0x55, 85
    Some(InitSSLot),    // 0x56, 86
    Some(InitSlot),     // 0x57, 87
    Some(LdSFLd0),      // 0x58, 88
    Some(LdSFLd1),      // 0x59, 89
    Some(LdSFLd2),      // 0x5A, 90
    Some(LdSFLd3),      // 0x5B, 91
    Some(LdSFLd4),      // 0x5C, 92
    Some(LdSFLd5),      // 0x5D, 93
    Some(LdSFLd6),      // 0x5E, 94
    Some(LdSFLd),       // 0x5F, 95
    Some(StSFLd0),      // 0x60, 96
    Some(StSFLd1),      // 0x61, 97
    Some(StSFLd2),      // 0x62, 98
    Some(StSFLd3),      // 0x63, 99
    Some(StSFLd4),      // 0x64, 100
    Some(StSFLd5),      // 0x65, 101
    Some(StSFLd6),      // 0x66, 102
    Some(StSFLd),       // 0x67, 103
    Some(LdLoc0),       // 0x68, 104
    Some(LdLoc1),       // 0x69, 105
    Some(LdLoc2),       // 0x6A, 106
    Some(LdLoc3),       // 0x6B, 107
    Some(LdLoc4),       // 0x6C, 108
    Some(LdLoc5),       // 0x6D, 109
    Some(LdLoc6),       // 0x6E, 110
    Some(LdLoc),        // 0x6F, 111
    Some(StLoc0),       // 0x70, 112
    Some(StLoc1),       // 0x71, 113
    Some(StLoc2),       // 0x72, 114
    Some(StLoc3),       // 0x73, 115
    Some(StLoc4),       // 0x74, 116
    Some(StLoc5),       // 0x75, 117
    Some(StLoc6),       // 0x76, 118
    Some(StLoc),        // 0x77, 119
    Some(LdArg0),       // 0x78, 120
    Some(LdArg1),       // 0x79, 121
    Some(LdArg2),       // 0x7A, 122
    Some(LdArg3),       // 0x7B, 123
    Some(LdArg4),       // 0x7C, 124
    Some(LdArg5),       // 0x7D, 125
    Some(LdArg6),       // 0x7E, 126
    Some(LdArg),        // 0x7F, 127
    Some(StArg0),       // 0x80, 128
    Some(StArg1),       // 0x81, 129
    Some(StArg2),       // 0x82, 130
    Some(StArg3),       // 0x83, 131
    Some(StArg4),       // 0x84, 132
    Some(StArg5),       // 0x85, 133
    Some(StArg6),       // 0x86, 134
    Some(StArg),        // 0x87, 135
    Some(NewBuffer),    // 0x88, 136
    Some(MemCpy),       // 0x89, 137
    None,               // Reserved
    Some(Cat),          // 0x8B, 139
    Some(SubStr),       // 0x8C, 140
    Some(Left),         // 0x8D, 141
    Some(Right),        // 0x8E, 142
    None,               // Reserved
    Some(Invert),       // 0x90, 144
    Some(And),          // 0x91, 145
    Some(Or),           // 0x92, 146
    Some(Xor),          // 0x93, 147
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    Some(Equal),        // 0x97, 151
    Some(NotEqual),     // 0x98, 152
    Some(Sign),         // 0x99, 153
    Some(Abs),          // 0x9A, 154
    Some(Negate),       // 0x9B, 155
    Some(Inc),          // 0x9C, 156
    Some(Dec),          // 0x9D, 157
    Some(Add),          // 0x9E, 158
    Some(Sub),          // 0x9F, 159
    Some(Mul),          // 0xA0, 160
    Some(Div),          // 0xA1, 161
    Some(Mod),          // 0xA2, 162
    Some(Pow),          // 0xA3, 163
    Some(Sqrt),         // 0xA4, 164
    Some(ModMul),       // 0xA5, 165
    Some(ModPow),       // 0xA6, 166
    None,               // Reserved
    Some(Shl),          // 0xA8, 168
    Some(Shr),          // 0xA9, 169
    Some(Not),          // 0xAA, 170
    Some(BoolAnd),      // 0xAB, 171
    Some(BoolOr),       // 0xAC, 172
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    Some(Nz),           // 0xB1, 177
    None,               // Reserved
    Some(NumEqual),     // 0xB3, 179
    Some(NumNotEqual),  // 0xB4, 180
    Some(Lt),           // 0xB5, 181
    Some(Le),           // 0xB6, 182
    Some(Gt),           // 0xB7, 183
    Some(Ge),           // 0xB8, 184
    Some(Min),          // 0xB9, 185
    Some(Max),          // 0xBA, 186
    Some(Within),       // 0xBB, 187
    None,               // Reserved
    None,               // Reserved
    Some(PackMap),      // 0xBE, 190
    Some(PackStruct),   // 0xBF, 191
    Some(Pack),         // 0xC0, 192
    Some(Unpack),       // 0xC1, 193
    Some(NewArray0),    // 0xC2, 194
    Some(NewArray),     // 0xC3, 195
    Some(NewArrayT),    // 0xC4, 196
    Some(NewStruct0),   // 0xC5, 197
    Some(NewStruct),    // 0xC6, 198
    None,               // Reserved
    Some(NewMap),       // 0xC8, 200
    None,               // Reserved
    Some(Size),         // 0xCA, 202
    Some(HasKey),       // 0xCB, 203
    Some(Keys),         // 0xCC, 204
    Some(Values),       // 0xCD, 205
    Some(PickItem),     // 0xCE, 206
    Some(Append),       // 0xCF, 207
    Some(SetItem),      // 0xD0, 208
    Some(ReverseItems), // 0xD1, 209
    Some(Remove),       // 0xD2, 210
    Some(ClearItems),   // 0xD3, 211
    Some(PopItem),      // 0xD4, 212
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    Some(IsNull),       // 0xD8, 216
    Some(IsType),       // 0xD9, 217
    None,               // Reserved
    Some(Convert),      // 0xDB, 219
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    Some(AbortMsg),     // 0xE0, 224
    Some(AssertMsg),    // 0xE1, 225
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
    None,               // Reserved
];

// #[derive(Debug, Clone, EnumIter)]
// #[repr(u8)]
// pub enum Instruction {
//     PushInt8 { operand: i8 },       // = 0x00,
//     PushInt16 { operand: i16 },     // = 0x01,
//     PushInt32 { operand: i32 },     // = 0x02,
//     PushInt64 { operand: i64 },     // = 0x03,
//     PushInt128 { operand: i128 },   // = 0x04,
//     PushInt256 { operand: I256 },   // = 0x05,
//     PushTrue,                       // = 0x08,
//     PushFalse,                      // = 0x09,
//     PushA,                          // = 0x0A,
//     PushNull,                       // = 0x0B,
//     PushData1 { operand: Operand }, // = 0x0C,
//     PushData2 { operand: Operand }, // = 0x0D,
//     PushData4 { operand: Operand }, // = 0x0E,
//     PushM1,                         // = 0x0F,
//     Push0,                          // = 0x10,
//     Push1,                          // = 0x11,
//     Push2,                          // = 0x12,
//     Push3,                          // = 0x13,
//     Push4,                          // = 0x14,
//     Push5,                          // = 0x15,
//     Push6,                          // = 0x16,
//     Push7,                          // = 0x17,
//     Push8,                          // = 0x18,
//     Push9,                          // = 0x19,
//     Push10,                         // = 0x1A,
//     Push11,                         // = 0x1B,
//     Push12,                         // = 0x1C,
//     Push13,                         // = 0x1D,
//     Push14,                         // = 0x1E,
//     Push15,                         // = 0x1F,
//     Push16,                         // = 0x20,
//
//     Nop,                                 // = 0x21,
//     Jmp { operand: i8 },                 // = 0x22,
//     JmpL { operand: i32 },               // = 0x23,
//     JmpIf { operand: i8 },               // = 0x24,
//     JmpIfL { operand: i32 },             // = 0x25,
//     JmpIfNot { operand: i8 },            // = 0x26,
//     JmpIfNotL { operand: i32 },          // = 0x27,
//     JmpEq { operand: i8 },               // = 0x28,
//     JmpEqL { operand: i32 },             // = 0x29,
//     JmpNe { operand: i8 },               // = 0x2A,
//     JmpNeL { operand: i32 },             // = 0x2B,
//     JmpGt { operand: i8 },               // = 0x2C,
//     JmpGtL { operand: i32 },             // = 0x2D,
//     JmpGe { operand: i8 },               // = 0x2E,
//     JmpGeL { operand: i32 },             // = 0x2F,
//     JmpLt { operand: i8 },               // = 0x30,
//     JmpLtL { operand: i32 },             // = 0x31,
//     JmpLe { operand: i8 },               // = 0x32,
//     JmpLeL { operand: i32 },             // = 0x33,
//     Call { operand: i8 },                // = 0x34,
//     CallL { operand: i32 },              // = 0x35,
//     CallA,                               // = 0x36,
//     CallT { operand: u16 },              // = 0x37,
//     Abort,                               // = 0x38,
//     Assert,                              // = 0x39,
//     Throw,                               // = 0x3A,
//     Try { catch: i8, finally: i8 },      // = 0x3B,
//     TryL { operand: i32, finally: i32 }, // = 0x3C,
//     EndTry { operand: i8 },              // = 0x3D,
//     EndTryL { operand: i32 },            // = 0x3E,
//     EndFinally,                          // = 0x3F,
//     Return,                              // = 0x40,
//
//     Syscall { operand: u32 },         // = 0x41,
//     Depth,                            // = 0x43,
//     Drop,                             // = 0x45,
//     Nip,                              // = 0x46,
//     Xdrop,                            // = 0x48,
//     Clear,                            // = 0x49,
//     Dup,                              // = 0x4A,
//     Over,                             // = 0x4B,
//     Pick,                             // = 0x4D,
//     Tuck,                             // = 0x4E,
//     Swap,                             // = 0x50,
//     Rotate,                           // = 0x51,
//     Roll,                             // = 0x52,
//     Reverse3,                         // = 0x53,
//     Reverse4,                         // = 0x54,
//     ReverseN,                         // = 0x55,
//     InitSSLot { operand: u8 },        // = 0x56,
//     InitSlot { slots: u8, vars: u8 }, // = 0x57,
//
//     LdSFLd0,                // = 0x58,
//     LdSFLd1,                // = 0x59,
//     LdSFLd2,                // = 0x5A,
//     LdSFLd3,                // = 0x5B,
//     LdSFLd4,                // = 0x5C,
//     LdSFLd5,                // = 0x5D,
//     LdSFLd6,                // = 0x5E,
//     LdSFLd { operand: u8 }, // = 0x5F,
//     StSFLd0,                // = 0x60,
//     StSFLd1,                // = 0x61,
//     StSFLd2,                // = 0x62,
//     StSFLd3,                // = 0x63,
//     StSFLd4,                // = 0x64,
//     StSFLd5,                // = 0x65,
//     StSFLd6,                // = 0x66,
//     StSFLd { operand: u8 }, // = 0x67,
//     LdLoc0,                 // = 0x68,
//     LdLoc1,                 // = 0x69,
//     LdLoc2,                 // = 0x6A,
//     LdLoc3,                 // = 0x6B,
//     LdLoc4,                 // = 0x6C,
//     LdLoc5,                 // = 0x6D,
//     LdLoc6,                 // = 0x6E,
//     LdLoc { operand: u8 },  // = 0x6F,
//     StLoc0,                 // = 0x70,
//     StLoc1,                 // = 0x71,
//     StLoc2,                 // = 0x72,
//     StLoc3,                 // = 0x73,
//     StLoc4,                 // = 0x74,
//     StLoc5,                 // = 0x75,
//     StLoc6,                 // = 0x76,
//     StLoc { operand: u8 },  // = 0x77,
//     LdArg0,                 // = 0x78,
//     LdArg1,                 // = 0x79,
//     LdArg2,                 // = 0x7A,
//     LdArg3,                 // = 0x7B,
//     LdArg4,                 // = 0x7C,
//     LdArg5,                 // = 0x7D,
//     LdArg6,                 // = 0x7E,
//     LdArg { operand: u8 },  // = 0x7F,
//     StArg0,                 // = 0x80,
//     StArg1,                 // = 0x81,
//     StArg2,                 // = 0x82,
//     StArg3,                 // = 0x83,
//     StArg4,                 // = 0x84,
//     StArg5,                 // = 0x85,
//     StArg6,                 // = 0x86,
//     StArg { operand: u8 },  // = 0x87,
//
//     NewBuffer,                 // = 0x88,
//     MemCpy,                    // = 0x89,
//     Cat,                       // = 0x8B,
//     SubStr,                    // = 0x8C,
//     Left,                      // = 0x8D,
//     Right,                     // = 0x8E,
//     Invert,                    // = 0x90,
//     And,                       // = 0x91,
//     Or,                        // = 0x92,
//     Xor,                       // = 0x93,
//     Equal,                     // = 0x97,
//     NotEqual,                  // = 0x98,
//     Sign,                      // = 0x99,
//     Abs,                       // = 0x9A,
//     Negate,                    // = 0x9B,
//     Inc,                       // = 0x9C,
//     Dec,                       // = 0x9D,
//     Add,                       // = 0x9E,
//     Sub,                       // = 0x9F,
//     Mul,                       // = 0xA0,
//     Div,                       // = 0xA1,
//     Mod,                       // = 0xA2,
//     Pow,                       // = 0xA3,
//     Sqrt,                      // = 0xA4,
//     ModMul,                    // = 0xA5,
//     ModPow,                    // = 0xA6,
//     Shl,                       // = 0xA8,
//     Shr,                       // = 0xA9,
//     Not,                       // = 0xAA,
//     BoolAnd,                   // = 0xAB,
//     BoolOr,                    // = 0xAC,
//     Nz,                        // = 0xB1,
//     NumEqual,                  // = 0xB3,
//     NumNotEqual,               // = 0xB4,
//     Lt,                        // = 0xB5,
//     Le,                        // = 0xB6,
//     Gt,                        // = 0xB7,
//     Ge,                        // = 0xB8,
//     Min,                       // = 0xB9,
//     Max,                       // = 0xBA,
//     Within,                    // = 0xBB,
//     PackMap,                   // = 0xBE,
//     PackStruct,                // = 0xBF,
//     Pack,                      // = 0xC0,
//     Unpack,                    // = 0xC1,
//     NewArray0,                 // = 0xC2,
//     NewArray,                  // = 0xC3,
//     NewArrayT { operand: u8 }, // = 0xC4,
//     NewStruct0,                // = 0xC5,
//     NewStruct,                 // = 0xC6,
//     NewMap,                    // = 0xC8,
//     Size,                      // = 0xCA,
//     HasKey,                    // = 0xCB,
//     Keys,                      // = 0xCC,
//     Values,                    // = 0xCD,
//     PickItem,                  // = 0xCE,
//     Append,                    // = 0xCF,
//     SetItem,                   // = 0xD0,
//     ReverseItems,              // = 0xD1,
//     Remove,                    // = 0xD2,
//     ClearItems,                // = 0xD3,
//     PopItem,                   // = 0xD4,
//     IsNull,                    // = 0xD8,
//     IsType { operand: u8 },    // = 0xD9,
//     Convert { operand: u8 },   // = 0xDB,
//     AbortMsg,                  // = 0xE0,
//     AssertMsg,                 // = 0xE1,
// }
