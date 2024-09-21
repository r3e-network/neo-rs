// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::{CodeAttr, OpCode, OpCode::*};

pub(crate) const CODE_ATTRS: [CodeAttr; 256] = [
    CodeAttr { price: 1, trailing: 1, unsigned: false }, // PushInt8, 0x00, 00
    CodeAttr { price: 1, trailing: 2, unsigned: false }, // PushInt16, 0x01, 01
    CodeAttr { price: 1, trailing: 4, unsigned: false }, // PushInt32, 0x02, 02
    CodeAttr { price: 1, trailing: 8, unsigned: false }, // PushInt64, 0x03, 03
    CodeAttr { price: 4, trailing: 16, unsigned: false }, // PushInt128, 0x04, 04
    CodeAttr { price: 4, trailing: 32, unsigned: false }, // PushInt256, 0x05, 05
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x06, 06
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x07, 07
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // PushTrue, 0x08, 08
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // PushFalse, 0x09, 09
    CodeAttr { price: 4, trailing: 4, unsigned: false }, // PushA, 0x0A, 10
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // PushNull, 0x0B, 11
    CodeAttr { price: 8, trailing: 1, unsigned: true },  // PushData1, 0x0C, 12
    CodeAttr { price: 512, trailing: 2, unsigned: true }, // PushData2, 0x0D, 13
    CodeAttr { price: 4096, trailing: 4, unsigned: true }, // PushData4, 0x0E, 14
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // PushM1, 0x0F, 15
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push0, 0x10, 16
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push1, 0x11, 17
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push2, 0x12, 18
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push3, 0x13, 19
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push4, 0x14, 20
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push5, 0x15, 21
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push6, 0x16, 22
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push7, 0x17, 23
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push8, 0x18, 24
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push9, 0x19, 25
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push10, 0x1A, 26
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push11, 0x1B, 27
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push12, 0x1C, 28
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push13, 0x1D, 29
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push14, 0x1E, 30
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push15, 0x1F, 31
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Push16, 0x20, 32
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Nop, 0x21, 33
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // Jmp, 0x22, 34
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // JmpL, 0x23, 35
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // JmpIf, 0x24, 36
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // JmpIfL, 0x25, 37
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // JmpIfNot, 0x26, 38
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // JmpIfNotL, 0x27, 39
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // JmpEq, 0x28, 40
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // JmpEqL, 0x29, 41
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // JmpNe, 0x2A, 42
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // JmpNeL, 0x2B, 43
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // JmpGt, 0x2C, 44
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // JmpGtL, 0x2D, 45
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // JmpGe, 0x2E, 46
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // JmpGeL, 0x2F, 47
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // JmpLt, 0x30, 48
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // JmpLtL, 0x31, 49
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // JmpLe, 0x32, 50
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // JmpLeL, 0x33, 51
    CodeAttr { price: 512, trailing: 1, unsigned: false }, // Call, 0x34, 52
    CodeAttr { price: 512, trailing: 4, unsigned: false }, // CallL, 0x35, 53
    CodeAttr { price: 512, trailing: 0, unsigned: false }, // CallA, 0x36, 54
    CodeAttr { price: 32768, trailing: 2, unsigned: true }, // CallT, 0x37, 55
    CodeAttr { price: 0, trailing: 0, unsigned: false }, // Abort, 0x38, 56
    CodeAttr { price: 1, trailing: 0, unsigned: false }, // Assert, 0x39, 57
    CodeAttr { price: 512, trailing: 0, unsigned: false }, // Throw, 0x3A, 58
    CodeAttr { price: 4, trailing: 2, unsigned: false }, // Try, 0x3B, 59
    CodeAttr { price: 2, trailing: 8, unsigned: false }, // TryL, 0x3C, 60
    CodeAttr { price: 2, trailing: 1, unsigned: false }, // EndTry, 0x3D, 61
    CodeAttr { price: 2, trailing: 4, unsigned: false }, // EndTryL, 0x3E, 62
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // EndFinally, 0x3F, 63
    CodeAttr { price: 0, trailing: 0, unsigned: false }, // Return, 0x40, 64
    CodeAttr { price: 0, trailing: 4, unsigned: true },  // Syscall, 0x41, 65
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x42, 66
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Depth, 0x43, 67
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x44, 68
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Drop, 0x45, 69
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Nip, 0x46, 70
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x47, 71
    CodeAttr { price: 16, trailing: 0, unsigned: false }, // Xdrop, 0x48, 72
    CodeAttr { price: 16, trailing: 0, unsigned: false }, // Clear, 0x49, 73
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Dup, 0x4A, 74
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Over, 0x4B, 75
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x4C, 76
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Pick, 0x4D, 77
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Tuck, 0x4E, 78
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x4F, 79
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Swap, 0x50, 80
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Rotate, 0x51, 81
    CodeAttr { price: 16, trailing: 0, unsigned: false }, // Roll, 0x52, 82
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reverse3, 0x53, 83
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reverse4, 0x54, 84
    CodeAttr { price: 16, trailing: 0, unsigned: false }, // ReverseN, 0x55, 85
    CodeAttr { price: 16, trailing: 1, unsigned: true }, // InitSSLot, 0x56, 86
    CodeAttr { price: 64, trailing: 2, unsigned: true }, // InitSlot, 0x57, 87
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdSFLd0, 0x58, 88
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdSFLd1, 0x59, 89
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdSFLd2, 0x5A, 90
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdSFLd3, 0x5B, 91
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdSFLd4, 0x5C, 92
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdSFLd5, 0x5D, 93
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdSFLd6, 0x5E, 94
    CodeAttr { price: 2, trailing: 1, unsigned: true },  // LdSFLd, 0x5F, 95
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StSFLd0, 0x60, 96
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StSFLd1, 0x61, 97
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StSFLd2, 0x62, 98
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StSFLd3, 0x63, 99
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StSFLd4, 0x64, 100
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StSFLd5, 0x65, 101
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StSFLd6, 0x66, 102
    CodeAttr { price: 2, trailing: 1, unsigned: true },  // StSFLd, 0x67, 103
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdLoc0, 0x68, 104
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdLoc1, 0x69, 105
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdLoc2, 0x6A, 106
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdLoc3, 0x6B, 107
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdLoc4, 0x6C, 108
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdLoc5, 0x6D, 109
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdLoc6, 0x6E, 110
    CodeAttr { price: 2, trailing: 1, unsigned: true },  // LdLoc, 0x6F, 111
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StLoc0, 0x70, 112
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StLoc1, 0x71, 113
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StLoc2, 0x72, 114
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StLoc3, 0x73, 115
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StLoc4, 0x74, 116
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StLoc5, 0x75, 117
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StLoc6, 0x76, 118
    CodeAttr { price: 2, trailing: 1, unsigned: true },  // StLoc, 0x77, 119
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdArg0, 0x78, 120
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdArg1, 0x79, 121
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdArg2, 0x7A, 122
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdArg3, 0x7B, 123
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdArg4, 0x7C, 124
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdArg5, 0x7D, 125
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // LdArg6, 0x7E, 126
    CodeAttr { price: 2, trailing: 1, unsigned: true },  // LdArg, 0x7F, 127
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StArg0, 0x80, 128
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StArg1, 0x81, 129
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StArg2, 0x82, 130
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StArg3, 0x83, 131
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StArg4, 0x84, 132
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StArg5, 0x85, 133
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // StArg6, 0x86, 134
    CodeAttr { price: 2, trailing: 1, unsigned: true },  // StArg, 0x87, 135
    CodeAttr { price: 256, trailing: 0, unsigned: false }, // NewBuffer, 0x88, 136
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // MemCpy, 0x89, 137
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x8A, 138
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // Cat, 0x8B, 139
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // SubStr, 0x8C, 140
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // Left, 0x8D, 141
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // Right, 0x8E, 142
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x8F, 143
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Invert, 0x90, 144
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // And, 0x91, 145
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Or, 0x92, 146
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Xor, 0x93, 147
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x94, 148
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x95, 149
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0x96, 150
    CodeAttr { price: 32, trailing: 0, unsigned: false }, // Equal, 0x97, 151
    CodeAttr { price: 32, trailing: 0, unsigned: false }, // NotEqual, 0x98, 152
    CodeAttr { price: 4, trailing: 0, unsigned: false }, // Sign, 0x99, 153
    CodeAttr { price: 4, trailing: 0, unsigned: false }, // Abs, 0x9A, 154
    CodeAttr { price: 4, trailing: 0, unsigned: false }, // Negate, 0x9B, 155
    CodeAttr { price: 4, trailing: 0, unsigned: false }, // Inc, 0x9C, 156
    CodeAttr { price: 4, trailing: 0, unsigned: false }, // Dec, 0x9D, 157
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Add, 0x9E, 158
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Sub, 0x9F, 159
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Mul, 0xA0, 160
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Div, 0xA1, 161
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Mod, 0xA2, 162
    CodeAttr { price: 64, trailing: 0, unsigned: false }, // Pow, 0xA3, 163
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // Sqrt, 0xA4, 164
    CodeAttr { price: 32, trailing: 0, unsigned: false }, // ModMul, 0xA5, 165
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // ModPow, 0xA6, 166
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xA7, 167
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Shl, 0xA8, 168
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Shr, 0xA9, 169
    CodeAttr { price: 4, trailing: 0, unsigned: false }, // Not, 0xAA, 170
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // BoolAnd, 0xAB, 171
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // BoolOr, 0xAC, 172
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xAD, 173
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xAE, 174
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xAF, 175
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xB0, 176
    CodeAttr { price: 4, trailing: 0, unsigned: false }, // Nz, 0xB1, 177
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xB2, 178
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // NumEqual, 0xB3, 179
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // NumNotEqual, 0xB4, 180
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Lt, 0xB5, 181
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Le, 0xB6, 182
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Gt, 0xB7, 183
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Ge, 0xB8, 184
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Min, 0xB9, 185
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Max, 0xBA, 186
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // Within, 0xBB, 187
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xBC, 188
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xBD, 189
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // PackMap, 0xBE, 190
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // PackStruct, 0xBF, 191
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // Pack, 0xC0, 192
    CodeAttr { price: 2048, trailing: 0, unsigned: false }, // Unpack, 0xC1, 193
    CodeAttr { price: 16, trailing: 0, unsigned: false }, // NewArray0, 0xC2, 194
    CodeAttr { price: 512, trailing: 0, unsigned: false }, // NewArray, 0xC3, 195
    CodeAttr { price: 512, trailing: 1, unsigned: true }, // NewArrayT, 0xC4, 196
    CodeAttr { price: 16, trailing: 0, unsigned: false }, // NewStruct0, 0xC5, 197
    CodeAttr { price: 512, trailing: 0, unsigned: false }, // NewStruct, 0xC6, 198
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xC7, 199
    CodeAttr { price: 8, trailing: 0, unsigned: false }, // NewMap, 0xC8, 200
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xC9, 201
    CodeAttr { price: 4, trailing: 0, unsigned: false }, // Size, 0xCA, 202
    CodeAttr { price: 64, trailing: 0, unsigned: false }, // HasKey, 0xCB, 203
    CodeAttr { price: 16, trailing: 0, unsigned: false }, // Keys, 0xCC, 204
    CodeAttr { price: 8192, trailing: 0, unsigned: false }, // Values, 0xCD, 205
    CodeAttr { price: 64, trailing: 0, unsigned: false }, // PickItem, 0xCE, 206
    CodeAttr { price: 8192, trailing: 0, unsigned: false }, // Append, 0xCF, 207
    CodeAttr { price: 8192, trailing: 0, unsigned: false }, // SetItem, 0xD0, 208
    CodeAttr { price: 8192, trailing: 0, unsigned: false }, // ReverseItems, 0xD1, 209
    CodeAttr { price: 16, trailing: 0, unsigned: false }, // Remove, 0xD2, 210
    CodeAttr { price: 16, trailing: 0, unsigned: false }, // ClearItems, 0xD3, 211
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // PopItem, 0xD4, 212
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xD5, 213
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xD6, 214
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xD7, 215
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // IsNull, 0xD8, 216
    CodeAttr { price: 2, trailing: 1, unsigned: true },  // IsType, 0xD9, 217
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xDA, 218
    CodeAttr { price: 8192, trailing: 1, unsigned: true }, // Convert, 0xDB, 219
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xDC, 220
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xDD, 221
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xDE, 222
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xDF, 223
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // AbortMsg, 0xE0, 224
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // AssertMsg, 0xE1, 225
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xE2, 226
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xE3, 227
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xE4, 228
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xE5, 229
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xE6, 230
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xE7, 231
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xE8, 232
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xE9, 233
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xEA, 234
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xEB, 235
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xEC, 236
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xED, 237
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xEE, 238
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xEF, 239
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF0, 240
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF1, 241
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF2, 242
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF3, 243
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF4, 244
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF5, 245
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF6, 246
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF7, 247
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF8, 248
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xF9, 249
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xFA, 250
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xFB, 251
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xFC, 252
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xFD, 253
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xFE, 254
    CodeAttr { price: 2, trailing: 0, unsigned: false }, // Reserved, 0xFF, 255
];

pub(crate) const OP_CODES: [Option<OpCode>; 256] = [
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
