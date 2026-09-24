#!/usr/bin/env python3
"""
NeoVM N3 Opcode Completeness Verification Script
Compares neo-rs implementation against official Neo N3 specification
"""

# Official Neo N3 OpCode specification (from Neo.VM 3.x)
# Source: https://github.com/neo-project/neo-vm/blob/master/src/Neo.VM/OpCode.cs

NEO_N3_OPCODES = {
    # Constants
    0x00: "PUSHINT8",
    0x01: "PUSHINT16",
    0x02: "PUSHINT32",
    0x03: "PUSHINT64",
    0x04: "PUSHINT128",
    0x05: "PUSHINT256",
    0x08: "PUSHT",
    0x09: "PUSHF",
    0x0A: "PUSHA",
    0x0B: "PUSHNULL",
    0x0C: "PUSHDATA1",
    0x0D: "PUSHDATA2",
    0x0E: "PUSHDATA4",
    0x0F: "PUSHM1",
    0x10: "PUSH0",
    0x11: "PUSH1",
    0x12: "PUSH2",
    0x13: "PUSH3",
    0x14: "PUSH4",
    0x15: "PUSH5",
    0x16: "PUSH6",
    0x17: "PUSH7",
    0x18: "PUSH8",
    0x19: "PUSH9",
    0x1A: "PUSH10",
    0x1B: "PUSH11",
    0x1C: "PUSH12",
    0x1D: "PUSH13",
    0x1E: "PUSH14",
    0x1F: "PUSH15",
    0x20: "PUSH16",

    # Flow control
    0x21: "NOP",
    0x22: "JMP",
    0x23: "JMP_L",
    0x24: "JMPIF",
    0x25: "JMPIF_L",
    0x26: "JMPIFNOT",
    0x27: "JMPIFNOT_L",
    0x28: "JMPEQ",
    0x29: "JMPEQ_L",
    0x2A: "JMPNE",
    0x2B: "JMPNE_L",
    0x2C: "JMPGT",
    0x2D: "JMPGT_L",
    0x2E: "JMPGE",
    0x2F: "JMPGE_L",
    0x30: "JMPLT",
    0x31: "JMPLT_L",
    0x32: "JMPLE",
    0x33: "JMPLE_L",
    0x34: "CALL",
    0x35: "CALL_L",
    0x36: "CALLA",
    0x37: "CALLT",
    0x38: "ABORT",
    0x39: "ASSERT",
    0x3A: "THROW",
    0x3B: "TRY",
    0x3C: "TRY_L",
    0x3D: "ENDTRY",
    0x3E: "ENDTRY_L",
    0x3F: "ENDFINALLY",
    0x40: "RET",
    0x41: "SYSCALL",

    # Stack
    0x43: "DEPTH",
    0x45: "DROP",
    0x46: "NIP",
    0x48: "XDROP",
    0x49: "CLEAR",
    0x4A: "DUP",
    0x4B: "OVER",
    0x4D: "PICK",
    0x4E: "TUCK",
    0x50: "SWAP",
    0x51: "ROT",
    0x52: "ROLL",
    0x53: "REVERSE3",
    0x54: "REVERSE4",
    0x55: "REVERSEN",

    # Slot
    0x56: "INITSSLOT",
    0x57: "INITSLOT",
    0x58: "LDSFLD0",
    0x59: "LDSFLD1",
    0x5A: "LDSFLD2",
    0x5B: "LDSFLD3",
    0x5C: "LDSFLD4",
    0x5D: "LDSFLD5",
    0x5E: "LDSFLD6",
    0x5F: "LDSFLD",
    0x60: "STSFLD0",
    0x61: "STSFLD1",
    0x62: "STSFLD2",
    0x63: "STSFLD3",
    0x64: "STSFLD4",
    0x65: "STSFLD5",
    0x66: "STSFLD6",
    0x67: "STSFLD",
    0x68: "LDLOC0",
    0x69: "LDLOC1",
    0x6A: "LDLOC2",
    0x6B: "LDLOC3",
    0x6C: "LDLOC4",
    0x6D: "LDLOC5",
    0x6E: "LDLOC6",
    0x6F: "LDLOC",
    0x70: "STLOC0",
    0x71: "STLOC1",
    0x72: "STLOC2",
    0x73: "STLOC3",
    0x74: "STLOC4",
    0x75: "STLOC5",
    0x76: "STLOC6",
    0x77: "STLOC",
    0x78: "LDARG0",
    0x79: "LDARG1",
    0x7A: "LDARG2",
    0x7B: "LDARG3",
    0x7C: "LDARG4",
    0x7D: "LDARG5",
    0x7E: "LDARG6",
    0x7F: "LDARG",
    0x80: "STARG0",
    0x81: "STARG1",
    0x82: "STARG2",
    0x83: "STARG3",
    0x84: "STARG4",
    0x85: "STARG5",
    0x86: "STARG6",
    0x87: "STARG",

    # Splice
    0x88: "NEWBUFFER",
    0x89: "MEMCPY",
    0x8B: "CAT",
    0x8C: "SUBSTR",
    0x8D: "LEFT",
    0x8E: "RIGHT",

    # Bitwise logic
    0x90: "INVERT",
    0x91: "AND",
    0x92: "OR",
    0x93: "XOR",
    0x97: "EQUAL",
    0x98: "NOTEQUAL",

    # Arithmetic
    0x99: "SIGN",
    0x9A: "ABS",
    0x9B: "NEGATE",
    0x9C: "INC",
    0x9D: "DEC",
    0x9E: "ADD",
    0x9F: "SUB",
    0xA0: "MUL",
    0xA1: "DIV",
    0xA2: "MOD",
    0xA3: "POW",
    0xA4: "SQRT",
    0xA5: "MODMUL",
    0xA6: "MODPOW",
    0xA8: "SHL",
    0xA9: "SHR",
    0xAA: "NOT",
    0xAB: "BOOLAND",
    0xAC: "BOOLOR",
    0xB1: "NZ",
    0xB3: "NUMEQUAL",
    0xB4: "NUMNOTEQUAL",
    0xB5: "LT",
    0xB6: "LE",
    0xB7: "GT",
    0xB8: "GE",
    0xB9: "MIN",
    0xBA: "MAX",
    0xBB: "WITHIN",

    # Compound types
    0xBE: "PACKMAP",
    0xBF: "PACKSTRUCT",
    0xC0: "PACK",
    0xC1: "UNPACK",
    0xC2: "NEWARRAY0",
    0xC3: "NEWARRAY",
    0xC4: "NEWARRAY_T",
    0xC5: "NEWSTRUCT0",
    0xC6: "NEWSTRUCT",
    0xC8: "NEWMAP",
    0xCA: "SIZE",
    0xCB: "HASKEY",
    0xCC: "KEYS",
    0xCD: "VALUES",
    0xCE: "PICKITEM",
    0xCF: "APPEND",
    0xD0: "SETITEM",
    0xD1: "REVERSEITEMS",
    0xD2: "REMOVE",
    0xD3: "CLEARITEMS",
    0xD4: "POPITEM",

    # Types
    0xD8: "ISNULL",
    0xD9: "ISTYPE",
    0xDB: "CONVERT",

    # Extensions
    0xE0: "ABORTMSG",
    0xE1: "ASSERTMSG",
}

def parse_neo_rs_opcodes(opcode_file_path):
    """Parse opcodes from neo-rs opcode.rs file"""
    opcodes = {}
    import re

    with open(opcode_file_path, 'r') as f:
        content = f.read()
        # Match patterns like: PUSHINT8 = 0x00, operand_size = 1, operand_prefix = 0;
        pattern = r'(\w+)\s*=\s*(0x[0-9A-Fa-f]+),'
        matches = re.findall(pattern, content)

        for name, byte_value in matches:
            byte_int = int(byte_value, 16)
            opcodes[byte_int] = name

    return opcodes

def main():
    import os

    # Parse neo-rs implementation
    opcode_file = "neo-vm/src/vm/opcode.rs"
    if not os.path.exists(opcode_file):
        print(f"ERROR: Cannot find {opcode_file}")
        return

    neo_rs_opcodes = parse_neo_rs_opcodes(opcode_file)

    print("=" * 80)
    print("NEO N3 NEOVM OPCODE COMPLETENESS VERIFICATION")
    print("=" * 80)
    print()

    # Check coverage
    print(f"Neo N3 Specification: {len(NEO_N3_OPCODES)} opcodes (0x00-0xFF range)")
    print(f"neo-rs Implementation: {len(neo_rs_opcodes)} opcodes")
    print()

    # Find missing opcodes
    missing = []
    for byte_val, name in sorted(NEO_N3_OPCODES.items()):
        if byte_val not in neo_rs_opcodes:
            missing.append((byte_val, name))

    # Find extra opcodes (shouldn't happen)
    extra = []
    for byte_val, name in sorted(neo_rs_opcodes.items()):
        if byte_val not in NEO_N3_OPCODES:
            extra.append((byte_val, name))

    # Find mismatched names
    mismatched = []
    for byte_val in sorted(set(NEO_N3_OPCODES.keys()) & set(neo_rs_opcodes.keys())):
        if NEO_N3_OPCODES[byte_val] != neo_rs_opcodes[byte_val]:
            mismatched.append((byte_val, NEO_N3_OPCODES[byte_val], neo_rs_opcodes[byte_val]))

    # Report missing opcodes
    if missing:
        print("[X] MISSING OPCODES:")
        print("=" * 80)
        for byte_val, name in missing:
            print(f"  0x{byte_val:02X}: {name}")
        print()
    else:
        print("[OK] All Neo N3 opcodes are defined")
        print()

    # Report extra opcodes
    if extra:
        print("[!] EXTRA OPCODES (not in Neo N3 spec):")
        print("=" * 80)
        for byte_val, name in extra:
            print(f"  0x{byte_val:02X}: {name}")
        print()

    # Report mismatched names
    if mismatched:
        print("[!] MISMATCHED OPCODE NAMES:")
        print("=" * 80)
        for byte_val, spec_name, impl_name in mismatched:
            print(f"  0x{byte_val:02X}: Spec={spec_name}, Impl={impl_name}")
        print()

    # Check coverage by category
    print("=" * 80)
    print("COVERAGE BY CATEGORY:")
    print("=" * 80)

    categories = {
        "Constants (0x00-0x20)": range(0x00, 0x21),
        "Flow Control (0x21-0x41)": range(0x21, 0x42),
        "Stack Operations (0x43-0x55)": range(0x43, 0x56),
        "Slot Operations (0x56-0x87)": range(0x56, 0x88),
        "Splice Operations (0x88-0x8E)": range(0x88, 0x8F),
        "Bitwise Logic (0x90-0x98)": range(0x90, 0x99),
        "Arithmetic (0x99-0xBB)": range(0x99, 0xBC),
        "Compound Types (0xBE-0xD4)": range(0xBE, 0xD5),
        "Type Operations (0xD8-0xDB)": range(0xD8, 0xDC),
        "Extensions (0xE0-0xE1)": range(0xE0, 0xE2),
    }

    for category_name, byte_range in categories.items():
        spec_opcodes_in_category = [b for b in byte_range if b in NEO_N3_OPCODES]
        impl_opcodes_in_category = [b for b in byte_range if b in neo_rs_opcodes]
        missing_in_category = [b for b in spec_opcodes_in_category if b not in neo_rs_opcodes]

        coverage = len(impl_opcodes_in_category) / len(spec_opcodes_in_category) * 100 if spec_opcodes_in_category else 100
        status = "[OK]" if coverage == 100 else "[X]"

        print(f"{status} {category_name}: {len(impl_opcodes_in_category)}/{len(spec_opcodes_in_category)} ({coverage:.1f}%)")

        if missing_in_category:
            for byte_val in missing_in_category:
                print(f"    Missing: 0x{byte_val:02X} {NEO_N3_OPCODES[byte_val]}")

    print()
    print("=" * 80)
    print("SUMMARY:")
    print("=" * 80)

    total_coverage = len(neo_rs_opcodes) / len(NEO_N3_OPCODES) * 100
    print(f"Overall Coverage: {len(neo_rs_opcodes)}/{len(NEO_N3_OPCODES)} ({total_coverage:.2f}%)")

    if not missing and not mismatched:
        print("[OK] Implementation is COMPLETE and CORRECT")
    elif not missing:
        print("[!] Implementation is COMPLETE but has naming mismatches")
    else:
        print(f"[X] Implementation is INCOMPLETE - {len(missing)} opcodes missing")

    print()

if __name__ == "__main__":
    main()
