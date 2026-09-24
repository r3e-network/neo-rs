#!/usr/bin/env python3
"""
NeoVM N3 Opcode Implementation Verification Script
Verifies that all defined opcodes are actually implemented in the interpreter
"""

import re
from collections import defaultdict

# All Neo N3 opcodes that should be implemented
ALL_OPCODES = [
    # Constants
    "PUSHINT8", "PUSHINT16", "PUSHINT32", "PUSHINT64", "PUSHINT128", "PUSHINT256",
    "PUSHT", "PUSHF", "PUSHA", "PUSHNULL", "PUSHDATA1", "PUSHDATA2", "PUSHDATA4",
    "PUSHM1", "PUSH0", "PUSH1", "PUSH2", "PUSH3", "PUSH4", "PUSH5", "PUSH6", "PUSH7",
    "PUSH8", "PUSH9", "PUSH10", "PUSH11", "PUSH12", "PUSH13", "PUSH14", "PUSH15", "PUSH16",

    # Flow control
    "NOP", "JMP", "JMP_L", "JMPIF", "JMPIF_L", "JMPIFNOT", "JMPIFNOT_L",
    "JMPEQ", "JMPEQ_L", "JMPNE", "JMPNE_L", "JMPGT", "JMPGT_L", "JMPGE", "JMPGE_L",
    "JMPLT", "JMPLT_L", "JMPLE", "JMPLE_L", "CALL", "CALL_L", "CALLA", "CALLT",
    "ABORT", "ASSERT", "THROW", "TRY", "TRY_L", "ENDTRY", "ENDTRY_L", "ENDFINALLY",
    "RET", "SYSCALL",

    # Stack
    "DEPTH", "DROP", "NIP", "XDROP", "CLEAR", "DUP", "OVER", "PICK", "TUCK",
    "SWAP", "ROT", "ROLL", "REVERSE3", "REVERSE4", "REVERSEN",

    # Slot
    "INITSSLOT", "INITSLOT",
    "LDSFLD0", "LDSFLD1", "LDSFLD2", "LDSFLD3", "LDSFLD4", "LDSFLD5", "LDSFLD6", "LDSFLD",
    "STSFLD0", "STSFLD1", "STSFLD2", "STSFLD3", "STSFLD4", "STSFLD5", "STSFLD6", "STSFLD",
    "LDLOC0", "LDLOC1", "LDLOC2", "LDLOC3", "LDLOC4", "LDLOC5", "LDLOC6", "LDLOC",
    "STLOC0", "STLOC1", "STLOC2", "STLOC3", "STLOC4", "STLOC5", "STLOC6", "STLOC",
    "LDARG0", "LDARG1", "LDARG2", "LDARG3", "LDARG4", "LDARG5", "LDARG6", "LDARG",
    "STARG0", "STARG1", "STARG2", "STARG3", "STARG4", "STARG5", "STARG6", "STARG",

    # Splice
    "NEWBUFFER", "MEMCPY", "CAT", "SUBSTR", "LEFT", "RIGHT",

    # Bitwise
    "INVERT", "AND", "OR", "XOR", "EQUAL", "NOTEQUAL",

    # Arithmetic
    "SIGN", "ABS", "NEGATE", "INC", "DEC", "ADD", "SUB", "MUL", "DIV", "MOD",
    "POW", "SQRT", "MODMUL", "MODPOW", "SHL", "SHR", "NOT", "BOOLAND", "BOOLOR",
    "NZ", "NUMEQUAL", "NUMNOTEQUAL", "LT", "LE", "GT", "GE", "MIN", "MAX", "WITHIN",

    # Compound types
    "PACKMAP", "PACKSTRUCT", "PACK", "UNPACK", "NEWARRAY0", "NEWARRAY", "NEWARRAY_T",
    "NEWSTRUCT0", "NEWSTRUCT", "NEWMAP", "SIZE", "HASKEY", "KEYS", "VALUES",
    "PICKITEM", "APPEND", "SETITEM", "REVERSEITEMS", "REMOVE", "CLEARITEMS", "POPITEM",

    # Types
    "ISNULL", "ISTYPE", "CONVERT",

    # Extensions
    "ABORTMSG", "ASSERTMSG",
]

def find_implemented_opcodes(executor_file):
    """Find which opcodes are handled in the main executor match statement"""
    implemented = set()

    with open(executor_file, 'r') as f:
        content = f.read()

    # Find all opcode references in match arms
    # Looking for patterns like: OPCODE | OPCODE2 => or OPCODE..=OPCODE2 =>
    patterns = [
        r'\b([A-Z_][A-Z0-9_]*)\s*=>',  # Single opcode
        r'\b([A-Z_][A-Z0-9_]*)\s*\|',   # Opcode in OR pattern
        r'\|\s*([A-Z_][A-Z0-9_]*)\s*=>', # Opcode after OR
        r'\b([A-Z_][A-Z0-9_]*)\.\.',    # Range start
        r'\.\.=?([A-Z_][A-Z0-9_]*)',    # Range end
    ]

    for pattern in patterns:
        matches = re.findall(pattern, content)
        for match in matches:
            if match in ALL_OPCODES:
                implemented.add(match)

    return implemented

def find_helper_module_opcodes(module_path, module_name):
    """Find opcodes handled by helper modules (push_ops, stack_ops, etc.)"""
    opcodes = set()

    try:
        with open(module_path, 'r') as f:
            content = f.read()

        # Look for match statements on opcode
        for opcode in ALL_OPCODES:
            if re.search(rf'\b{opcode}\b', content):
                opcodes.add(opcode)
    except:
        pass

    return opcodes

def main():
    print("=" * 80)
    print("NEO N3 NEOVM OPCODE IMPLEMENTATION VERIFICATION")
    print("=" * 80)
    print()

    # Main executor file
    executor_file = "neo-vm/src/interpreter/executor/mod.rs"

    # Find implemented opcodes in main dispatcher
    main_implemented = find_implemented_opcodes(executor_file)

    # Check helper modules
    helper_modules = {
        "push_ops": "neo-vm/src/interpreter/executor/push_ops.rs",
        "stack_ops": "neo-vm/src/interpreter/executor/stack_ops.rs",
        "slot_ops": "neo-vm/src/interpreter/executor/slot_ops.rs",
        "numeric_ops": "neo-vm/src/interpreter/executor/numeric_ops.rs",
        "byte_ops": "neo-vm/src/interpreter/executor/byte_ops.rs",
        "compound_ops": "neo-vm/src/interpreter/executor/compound_ops.rs",
        "control": "neo-vm/src/interpreter/executor/control.rs",
    }

    helper_implemented = defaultdict(set)
    for module_name, module_path in helper_modules.items():
        try:
            opcodes = find_helper_module_opcodes(module_path, module_name)
            helper_implemented[module_name] = opcodes
            main_implemented.update(opcodes)
        except Exception as e:
            print(f"Warning: Could not process {module_name}: {e}")

    # Find missing opcodes
    all_opcodes_set = set(ALL_OPCODES)
    missing = sorted(all_opcodes_set - main_implemented)
    implemented = sorted(main_implemented & all_opcodes_set)

    print(f"Total opcodes defined: {len(ALL_OPCODES)}")
    print(f"Opcodes with implementation: {len(implemented)}")
    print(f"Opcodes potentially missing: {len(missing)}")
    print()

    if missing:
        print("[!] POTENTIALLY UNIMPLEMENTED OPCODES:")
        print("=" * 80)
        print("Note: These opcodes may be implemented via range patterns (e.g., PUSH1..=PUSH16)")
        print()

        # Group by category
        categories = {
            "Constants": ["PUSHINT8", "PUSHINT16", "PUSHINT32", "PUSHINT64", "PUSHINT128", "PUSHINT256",
                         "PUSHT", "PUSHF", "PUSHA", "PUSHNULL", "PUSHDATA1", "PUSHDATA2", "PUSHDATA4",
                         "PUSHM1", "PUSH0", "PUSH1", "PUSH2", "PUSH3", "PUSH4", "PUSH5", "PUSH6", "PUSH7",
                         "PUSH8", "PUSH9", "PUSH10", "PUSH11", "PUSH12", "PUSH13", "PUSH14", "PUSH15", "PUSH16"],
            "Flow Control": ["NOP", "JMP", "JMP_L", "JMPIF", "JMPIF_L", "JMPIFNOT", "JMPIFNOT_L",
                           "JMPEQ", "JMPEQ_L", "JMPNE", "JMPNE_L", "JMPGT", "JMPGT_L", "JMPGE", "JMPGE_L",
                           "JMPLT", "JMPLT_L", "JMPLE", "JMPLE_L", "CALL", "CALL_L", "CALLA", "CALLT",
                           "ABORT", "ASSERT", "THROW", "TRY", "TRY_L", "ENDTRY", "ENDTRY_L", "ENDFINALLY",
                           "RET", "SYSCALL"],
            "Stack": ["DEPTH", "DROP", "NIP", "XDROP", "CLEAR", "DUP", "OVER", "PICK", "TUCK",
                     "SWAP", "ROT", "ROLL", "REVERSE3", "REVERSE4", "REVERSEN"],
            "Slot": ["INITSSLOT", "INITSLOT",
                    "LDSFLD0", "LDSFLD1", "LDSFLD2", "LDSFLD3", "LDSFLD4", "LDSFLD5", "LDSFLD6", "LDSFLD",
                    "STSFLD0", "STSFLD1", "STSFLD2", "STSFLD3", "STSFLD4", "STSFLD5", "STSFLD6", "STSFLD",
                    "LDLOC0", "LDLOC1", "LDLOC2", "LDLOC3", "LDLOC4", "LDLOC5", "LDLOC6", "LDLOC",
                    "STLOC0", "STLOC1", "STLOC2", "STLOC3", "STLOC4", "STLOC5", "STLOC6", "STLOC",
                    "LDARG0", "LDARG1", "LDARG2", "LDARG3", "LDARG4", "LDARG5", "LDARG6", "LDARG",
                    "STARG0", "STARG1", "STARG2", "STARG3", "STARG4", "STARG5", "STARG6", "STARG"],
            "Splice": ["NEWBUFFER", "MEMCPY", "CAT", "SUBSTR", "LEFT", "RIGHT"],
            "Bitwise": ["INVERT", "AND", "OR", "XOR", "EQUAL", "NOTEQUAL"],
            "Arithmetic": ["SIGN", "ABS", "NEGATE", "INC", "DEC", "ADD", "SUB", "MUL", "DIV", "MOD",
                          "POW", "SQRT", "MODMUL", "MODPOW", "SHL", "SHR", "NOT", "BOOLAND", "BOOLOR",
                          "NZ", "NUMEQUAL", "NUMNOTEQUAL", "LT", "LE", "GT", "GE", "MIN", "MAX", "WITHIN"],
            "Compound": ["PACKMAP", "PACKSTRUCT", "PACK", "UNPACK", "NEWARRAY0", "NEWARRAY", "NEWARRAY_T",
                        "NEWSTRUCT0", "NEWSTRUCT", "NEWMAP", "SIZE", "HASKEY", "KEYS", "VALUES",
                        "PICKITEM", "APPEND", "SETITEM", "REVERSEITEMS", "REMOVE", "CLEARITEMS", "POPITEM"],
            "Types": ["ISNULL", "ISTYPE", "CONVERT"],
            "Extensions": ["ABORTMSG", "ASSERTMSG"],
        }

        for category, opcodes in categories.items():
            missing_in_category = [op for op in opcodes if op in missing]
            if missing_in_category:
                print(f"\n{category}:")
                for op in missing_in_category:
                    print(f"  - {op}")

    print()
    print("=" * 80)
    print("IMPLEMENTATION STATUS BY CATEGORY:")
    print("=" * 80)

    categories = {
        "Constants": ["PUSHINT8", "PUSHINT16", "PUSHINT32", "PUSHINT64", "PUSHINT128", "PUSHINT256",
                     "PUSHT", "PUSHF", "PUSHA", "PUSHNULL", "PUSHDATA1", "PUSHDATA2", "PUSHDATA4",
                     "PUSHM1", "PUSH0", "PUSH1", "PUSH2", "PUSH3", "PUSH4", "PUSH5", "PUSH6", "PUSH7",
                     "PUSH8", "PUSH9", "PUSH10", "PUSH11", "PUSH12", "PUSH13", "PUSH14", "PUSH15", "PUSH16"],
        "Flow Control": ["NOP", "JMP", "JMP_L", "JMPIF", "JMPIF_L", "JMPIFNOT", "JMPIFNOT_L",
                       "JMPEQ", "JMPEQ_L", "JMPNE", "JMPNE_L", "JMPGT", "JMPGT_L", "JMPGE", "JMPGE_L",
                       "JMPLT", "JMPLT_L", "JMPLE", "JMPLE_L", "CALL", "CALL_L", "CALLA", "CALLT",
                       "ABORT", "ASSERT", "THROW", "TRY", "TRY_L", "ENDTRY", "ENDTRY_L", "ENDFINALLY",
                       "RET", "SYSCALL"],
        "Stack": ["DEPTH", "DROP", "NIP", "XDROP", "CLEAR", "DUP", "OVER", "PICK", "TUCK",
                 "SWAP", "ROT", "ROLL", "REVERSE3", "REVERSE4", "REVERSEN"],
        "Slot": ["INITSSLOT", "INITSLOT",
                "LDSFLD0", "LDSFLD1", "LDSFLD2", "LDSFLD3", "LDSFLD4", "LDSFLD5", "LDSFLD6", "LDSFLD",
                "STSFLD0", "STSFLD1", "STSFLD2", "STSFLD3", "STSFLD4", "STSFLD5", "STSFLD6", "STSFLD",
                "LDLOC0", "LDLOC1", "LDLOC2", "LDLOC3", "LDLOC4", "LDLOC5", "LDLOC6", "LDLOC",
                "STLOC0", "STLOC1", "STLOC2", "STLOC3", "STLOC4", "STLOC5", "STLOC6", "STLOC",
                "LDARG0", "LDARG1", "LDARG2", "LDARG3", "LDARG4", "LDARG5", "LDARG6", "LDARG",
                "STARG0", "STARG1", "STARG2", "STARG3", "STARG4", "STARG5", "STARG6", "STARG"],
        "Splice": ["NEWBUFFER", "MEMCPY", "CAT", "SUBSTR", "LEFT", "RIGHT"],
        "Bitwise": ["INVERT", "AND", "OR", "XOR", "EQUAL", "NOTEQUAL"],
        "Arithmetic": ["SIGN", "ABS", "NEGATE", "INC", "DEC", "ADD", "SUB", "MUL", "DIV", "MOD",
                      "POW", "SQRT", "MODMUL", "MODPOW", "SHL", "SHR", "NOT", "BOOLAND", "BOOLOR",
                      "NZ", "NUMEQUAL", "NUMNOTEQUAL", "LT", "LE", "GT", "GE", "MIN", "MAX", "WITHIN"],
        "Compound": ["PACKMAP", "PACKSTRUCT", "PACK", "UNPACK", "NEWARRAY0", "NEWARRAY", "NEWARRAY_T",
                    "NEWSTRUCT0", "NEWSTRUCT", "NEWMAP", "SIZE", "HASKEY", "KEYS", "VALUES",
                    "PICKITEM", "APPEND", "SETITEM", "REVERSEITEMS", "REMOVE", "CLEARITEMS", "POPITEM"],
        "Types": ["ISNULL", "ISTYPE", "CONVERT"],
        "Extensions": ["ABORTMSG", "ASSERTMSG"],
    }

    for category, opcodes in categories.items():
        implemented_in_category = [op for op in opcodes if op in main_implemented]
        missing_in_category = [op for op in opcodes if op not in main_implemented]
        coverage = len(implemented_in_category) / len(opcodes) * 100 if opcodes else 100
        status = "[OK]" if coverage == 100 else "[!]"
        print(f"{status} {category}: {len(implemented_in_category)}/{len(opcodes)} ({coverage:.1f}%)")

    print()
    print("=" * 80)
    print("FINAL ASSESSMENT:")
    print("=" * 80)

    coverage = len(implemented) / len(ALL_OPCODES) * 100
    print(f"Implementation Coverage: {len(implemented)}/{len(ALL_OPCODES)} ({coverage:.1f}%)")

    if len(missing) == 0:
        print("[OK] All opcodes appear to be implemented")
    else:
        print(f"[!] {len(missing)} opcodes may need verification")
        print("    (Some may be implemented via range patterns like PUSH1..=PUSH16)")

    print()

if __name__ == "__main__":
    main()
