// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use crate::RunPrice;

const PRICES: [u64; 255] = [
    1, // PushInt8, 0x0, 0
    1, // PushInt16, 0x1, 1
    1, // PushInt32, 0x2, 2
    1, // PushInt64, 0x3, 3
    4, // PushInt128, 0x4, 4
    4, // PushInt256, 0x5, 5
    2, // Reserved
    2, // Reserved
    2, // PushTrue, 0x8, 8
    2, // PushFalse, 0x9, 9
    4, // PushA, 0xA, 10
    1, // PushNull, 0xB, 11
    8, // PushData1, 0xC, 12
    512, // PushData2, 0xD, 13
    4096, // PushData4, 0xE, 14
    1, // PushM1, 0xF, 15
    1, // Push0, 0x10, 16
    1, // Push1, 0x11, 17
    1, // Push2, 0x12, 18
    1, // Push3, 0x13, 19
    1, // Push4, 0x14, 20
    1, // Push5, 0x15, 21
    1, // Push6, 0x16, 22
    1, // Push7, 0x17, 23
    1, // Push8, 0x18, 24
    1, // Push9, 0x19, 25
    1, // Push10, 0x1A, 26
    1, // Push11, 0x1B, 27
    1, // Push12, 0x1C, 28
    1, // Push13, 0x1D, 29
    1, // Push14, 0x1E, 30
    1, // Push15, 0x1F, 31
    1, // Push16, 0x20, 32
    1, // Nop, 0x21, 33
    2, // Jmp, 0x22, 34
    2, // JmpL, 0x23, 35
    2, // JmpIf, 0x24, 36
    2, // JmpIfL, 0x25, 37
    2, // JmpIfNot, 0x26, 38
    2, // JmpIfNotL, 0x27, 39
    2, // JmpEq, 0x28, 40
    2, // JmpEqL, 0x29, 41
    2, // JmpNe, 0x2A, 42
    2, // JmpNeL, 0x2B, 43
    2, // JmpGt, 0x2C, 44
    2, // JmpGtL, 0x2D, 45
    2, // JmpGe, 0x2E, 46
    2, // JmpGeL, 0x2F, 47
    2, // JmpLt, 0x30, 48
    2, // JmpLtL, 0x31, 49
    2, // JmpLe, 0x32, 50
    2, // JmpLeL, 0x33, 51
    512, // Call, 0x34, 52
    512, // CallL, 0x35, 53
    512, // CallA, 0x36, 54
    32768, // CallT, 0x37, 55
    0, // Abort, 0x38, 56
    1, // Assert, 0x39, 57
    512, // Throw, 0x3A, 58
    4, // Try, 0x3B, 59
    2, // TryL, 0x3C, 60
    2, // EndTry, 0x3D, 61
    2, // EndTryL, 0x3E, 62
    2, // EndFinally, 0x3F, 63
    0, // Ret, 0x40, 64
    0, // Syscall, 0x41, 65
    2, // Reserved
    2, // Depth, 0x43, 67
    2, // Reserved
    2, // Drop, 0x45, 69
    2, // Nip, 0x46, 70
    2, // Reserved
    16, // Xdrop, 0x48, 72
    16, // Clear, 0x49, 73
    2, // Dup, 0x4A, 74
    2, // Over, 0x4B, 75
    2, // Reserved
    2, // Pick, 0x4D, 77
    2, // Tuck, 0x4E, 78
    2, // Reserved
    2, // Swap, 0x50, 80
    2, // Rot, 0x51, 81
    16, // Roll, 0x52, 82
    2, // Reverse3, 0x53, 83
    2, // Reverse4, 0x54, 84
    16, // ReverseN, 0x55, 85
    16, // InitSSLot, 0x56, 86
    64, // InitSlot, 0x57, 87
    2, // LdSFLd0, 0x58, 88
    2, // LdSFLd1, 0x59, 89
    2, // LdSFLd2, 0x5A, 90
    2, // LdSFLd3, 0x5B, 91
    2, // LdSFLd4, 0x5C, 92
    2, // LdSFLd5, 0x5D, 93
    2, // LdSFLd6, 0x5E, 94
    2, // LdSFLd, 0x5F, 95
    2, // StSFLd0, 0x60, 96
    2, // StSFLd1, 0x61, 97
    2, // StSFLd2, 0x62, 98
    2, // StSFLd3, 0x63, 99
    2, // StSFLd4, 0x64, 100
    2, // StSFLd5, 0x65, 101
    2, // StSFLd6, 0x66, 102
    2, // StSFLd, 0x67, 103
    2, // LdLoc0, 0x68, 104
    2, // LdLoc1, 0x69, 105
    2, // LdLoc2, 0x6A, 106
    2, // LdLoc3, 0x6B, 107
    2, // LdLoc4, 0x6C, 108
    2, // LdLoc5, 0x6D, 109
    2, // LdLoc6, 0x6E, 110
    2, // LdLoc, 0x6F, 111
    2, // StLoc0, 0x70, 112
    2, // StLoc1, 0x71, 113
    2, // StLoc2, 0x72, 114
    2, // StLoc3, 0x73, 115
    2, // StLoc4, 0x74, 116
    2, // StLoc5, 0x75, 117
    2, // StLoc6, 0x76, 118
    2, // StLoc, 0x77, 119
    2, // LdArg0, 0x78, 120
    2, // LdArg1, 0x79, 121
    2, // LdArg2, 0x7A, 122
    2, // LdArg3, 0x7B, 123
    2, // LdArg4, 0x7C, 124
    2, // LdArg5, 0x7D, 125
    2, // LdArg6, 0x7E, 126
    2, // LdArg, 0x7F, 127
    2, // StArg0, 0x80, 128
    2, // StArg1, 0x81, 129
    2, // StArg2, 0x82, 130
    2, // StArg3, 0x83, 131
    2, // StArg4, 0x84, 132
    2, // StArg5, 0x85, 133
    2, // StArg6, 0x86, 134
    2, // StArg, 0x87, 135
    256, // NewBuffer, 0x88, 136
    2048, // MemCpy, 0x89, 137
    2,  // Reserved
    2048, // Cat, 0x8B, 139
    2048, // Substr, 0x8C, 140
    2048, // Left, 0x8D, 141
    2048, // Right, 0x8E, 142
    2,  // Reserved
    2, // Invert, 0x90, 144
    8, // And, 0x91, 145
    8, // Or, 0x92, 146
    8, // Xor, 0x93, 147
    2, // Reserved
    2, // Reserved
    2, // Reserved
    32, // Equal, 0x97, 151
    32, // NotEqual, 0x98, 152
    4, // Sign, 0x99, 153
    4, // Abs, 0x9A, 154
    4, // Negate, 0x9B, 155
    4, // Inc, 0x9C, 156
    4, // Dec, 0x9D, 157
    8, // Add, 0x9E, 158
    8, // Sub, 0x9F, 159
    8, // Mul, 0xA0, 160
    8, // Div, 0xA1, 161
    8, // Mod, 0xA2, 162
    64, // Pow, 0xA3, 163
    2048, // Sqrt, 0xA4, 164
    32, // ModMul, 0xA5, 165
    2048, // ModPow, 0xA6, 166
    2,  // Reserved
    8, // Shl, 0xA8, 168
    8, // Shr, 0xA9, 169
    4, // Not, 0xAA, 170
    8, // BoolAnd, 0xAB, 171
    8, // BoolOr, 0xAC, 172
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    4, // Nz, 0xB1, 177
    2, // Reserved
    8, // NumEqual, 0xB3, 179
    8, // NumNotEqual, 0xB4, 180
    8, // Lt, 0xB5, 181
    8, // Le, 0xB6, 182
    8, // Gt, 0xB7, 183
    8, // Ge, 0xB8, 184
    8, // Min, 0xB9, 185
    8, // Max, 0xBA, 186
    8, // Within, 0xBB, 187
    2,  // Reserved
    2,  // Reserved
    2048, // PackMap, 0xBE, 190
    2048, // PackStruct, 0xBF, 191
    2048, // Pack, 0xC0, 192
    2048, // Unpack, 0xC1, 193
    16, // NewArray0, 0xC2, 194
    512, // NewArray, 0xC3, 195
    512, // NewArrayT, 0xC4, 196
    16, // NewStruct0, 0xC5, 197
    512, // NewStruct, 0xC6, 198
    2,  // Reserved
    8, // NewMap, 0xC8, 200
    2, // Reserved
    4, // Size, 0xCA, 202
    64, // HasKey, 0xCB, 203
    16, // Keys, 0xCC, 204
    8192, // Values, 0xCD, 205
    64, // PickItem, 0xCE, 206
    8192, // Append, 0xCF, 207
    8192, // SetItem, 0xD0, 208
    8192, // ReverseItems, 0xD1, 209
    16, // Remove, 0xD2, 210
    16, // ClearItems, 0xD3, 211
    2, // PopItem, 0xD4, 212
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // IsNull, 0xD8, 216
    2, // IsType, 0xD9, 217
    2, // Reserved
    8192, // Convert, 0xDB, 219
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // AbortMsg, 0xE0, 224
    2, // AssertMsg, 0xE1, 225
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
    2, // Reserved
];

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
#[repr(u8)]
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
    SubStr = 0x8C,
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

impl RunPrice for OpCode {
    #[inline]
    fn price(&self) -> u64 { PRICES[*self as usize] }
}
