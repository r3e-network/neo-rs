"""Check coverage between the reference C# Neo node and the Rust port.

The script walks through all C# source files under neo_csharp/src and attempts
to infer the corresponding Rust module path by applying a set of heuristics and
naming conversions (CamelCase -> snake_case, dots -> module nesting, etc.). It
skips obvious C#-only artifacts (e.g. AssemblyInfo.cs) and reports the files for
which it cannot locate a matching Rust source file (excluding lib.rs / mod.rs as
requested by the user).

The goal is to provide a reproducible report of gaps so we can methodically
bring the Rust implementation to structural parity with the C# baseline.
"""
from __future__ import annotations

import argparse
import json
import re
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Sequence

REPO_ROOT = Path(__file__).resolve().parent.parent
C_SHARP_ROOT = REPO_ROOT / "neo_csharp" / "src"
RUST_ROOTS = [REPO_ROOT / "crates", REPO_ROOT / "src", REPO_ROOT / "node"]

# Exclusions for C# files that have no Rust analogue by design (e.g. assembly metadata).
C_SHARP_EXCLUDES = {
    "Properties/AssemblyInfo.cs",
    "Neo.ConsoleService/Properties/AssemblyInfo.cs",
    "Neo.Cryptography.BLS12_381/Properties/AssemblyInfo.cs",
    "Neo.Json/Properties/AssemblyInfo.cs",
    "Neo.VM/Properties/AssemblyInfo.cs",
    "Neo.VM/GlobalSuppressions.cs",
}


@dataclass(frozen=True)
class MappingRule:
    csharp_prefix: str
    rust_prefix: str

    def matches(self, rel_path: str) -> bool:
        return rel_path.startswith(self.csharp_prefix)

    def project(self, tail_segments: Sequence[str]) -> Path:
        target = Path(self.rust_prefix)
        for segment in tail_segments:
            parts = segment_to_parts(segment)
            for part in parts:
                target /= part
        return target


# Ordered from most specific to least so longer prefixes win first.
MAPPING_RULES: List[MappingRule] = [
    MappingRule("Neo/SmartContract/Native", "crates/smart_contract/src/native"),
    MappingRule("Neo/SmartContract/Manifest", "crates/smart_contract/src/manifest"),
    MappingRule("Neo/SmartContract/ApplicationEngine", "crates/smart_contract/src/application_engine"),
    MappingRule("Neo/SmartContract/Interop", "crates/smart_contract/src/interop"),
    MappingRule("Neo/SmartContract/Iterators", "crates/smart_contract/src/iterators"),
    MappingRule("Neo/SmartContract/Json", "crates/smart_contract/src"),
    MappingRule("Neo/SmartContract", "crates/smart_contract/src"),
    MappingRule("Neo/Network/P2P/Payloads/Conditions", "crates/network/src/messages/conditions"),
    MappingRule("Neo/Network/P2P/Payloads", "crates/network/src/payloads"),
    MappingRule("Neo/Network/P2P/Capabilities", "crates/network/src/messages/capabilities"),
    MappingRule("Neo/Network/P2P", "crates/network/src/p2p"),
    MappingRule("Neo/Network", "crates/network/src"),
    MappingRule("Neo/Ledger", "crates/ledger/src"),
    MappingRule("Neo/Persistence", "crates/persistence/src"),
    MappingRule("Neo/Wallets", "crates/wallets/src"),
    MappingRule("Neo/Cryptography/ECC", "crates/cryptography/src/ecc"),
    MappingRule("Neo/Cryptography", "crates/cryptography/src"),
    MappingRule("Neo.Extensions/SmartContract", "crates/extensions/src/smart_contract"),
    MappingRule("Neo/Extensions/SmartContract", "crates/extensions/src/smart_contract"),
    MappingRule("Neo.Extensions/Collections", "crates/extensions/src/collections"),
    MappingRule("Neo/Extensions/Collections", "crates/extensions/src/collections"),
    MappingRule("Neo.Extensions/Net", "crates/extensions/src/net"),
    MappingRule("Neo/Extensions/Net", "crates/extensions/src/net"),
    MappingRule("Neo.Extensions/Factories", "crates/extensions/src/factories"),
    MappingRule("Neo/Extensions/Factories", "crates/extensions/src/factories"),
    MappingRule("Neo.Extensions/Exceptions", "crates/extensions/src/exceptions"),
    MappingRule("Neo/Extensions/Exceptions", "crates/extensions/src/exceptions"),
    MappingRule("Neo.Extensions/VM", "crates/extensions/src/vm"),
    MappingRule("Neo/Extensions/VM", "crates/extensions/src/vm"),
    MappingRule("Neo.Extensions/IO", "crates/extensions/src/io"),
    MappingRule("Neo/Extensions/IO", "crates/extensions/src/io"),
    MappingRule("Neo.Extensions", "crates/extensions/src"),
    MappingRule("Neo/Extensions", "crates/extensions/src"),
    MappingRule("Neo.IO/Caching", "crates/io/src/caching"),
    MappingRule("Neo/IO/Caching", "crates/io/src/caching"),
    MappingRule("Neo.IO/Actors", "crates/io/src/actors"),
    MappingRule("Neo/IO/Actors", "crates/io/src/actors"),
    MappingRule("Neo.IO", "crates/io/src"),
    MappingRule("Neo/IO", "crates/io/src"),
    MappingRule("Neo/Builders", "crates/core/src/builders"),
    MappingRule("Neo/Sign", "crates/core/src"),
    MappingRule("Neo/IEventHandlers", "crates/core/src/event_handlers"),
    MappingRule("Neo.ConsoleService", "crates/cli/src"),
    MappingRule("Neo.CLI", "crates/cli/src"),
    MappingRule("Neo.Cryptography.BLS12_381", "crates/bls12_381/src"),
    MappingRule("Neo.Cryptography.MPTTrie", "crates/mpt_trie/src"),
    MappingRule("Neo.Json", "crates/json/src"),
    MappingRule("Neo.VM/JumpTable", "crates/vm/src/jump_table"),
    MappingRule("Neo.VM/Types", "crates/vm/src/stack_item"),
    MappingRule("Neo.VM/Collections", "crates/json/src"),
    MappingRule("Neo.VM", "crates/vm/src"),
    MappingRule("RpcClient", "crates/rpc_client/src"),
    MappingRule("Plugins/DBFTPlugin", "crates/plugins/src/dbft_plugin"),
    MappingRule("Plugins/RpcServer", "crates/plugins/src/rpc_server"),
    MappingRule("Plugins/RestServer", "crates/plugins/src/rest_server"),
    MappingRule("Plugins/OracleService", "crates/plugins/src/oracle_service"),
    MappingRule("Plugins/TokensTracker", "crates/plugins/src/tokens_tracker"),
    MappingRule("Plugins/SignClient", "crates/plugins/src/sign_client"),
    MappingRule("Plugins/SQLiteWallet", "crates/plugins/src/sqlite_wallet"),
    MappingRule("Plugins/StateService", "crates/plugins/src/state_service"),
    MappingRule("Plugins/ApplicationLogs", "crates/plugins/src/application_logs"),
    MappingRule("Plugins/StorageDumper", "crates/plugins/src/storage_dumper"),
    MappingRule("Plugins/LevelDBStore", "crates/plugins/src/leveldb_store"),
    MappingRule("Plugins/RocksDBStore", "crates/plugins/src/rocksdb_store"),
    MappingRule("Plugins", "crates/plugins/src"),
    MappingRule("Neo", "crates/core/src"),
]

FILE_OVERRIDES = {
    "Neo.VM/BadScriptException.cs": Path('crates/vm/src/bad_script_exception.rs'),
    "Neo.VM/CatchableException.cs": Path('crates/vm/src/catchable_exception.rs'),
    "Neo.VM/ExecutionEngineLimits.cs": Path('crates/vm/src/execution_engine_limits.rs'),
    "Neo.VM/VMState.cs": Path('crates/vm/src/vm_state.rs'),
    "Neo.VM/IReferenceCounter.cs": Path('crates/vm/src/i_reference_counter.rs'),
    "Neo.VM/Slot.cs": Path('crates/vm/src/slot.rs'),
    "Neo.VM/JumpTable/JumpTable.Bitwisee.cs": Path('crates/vm/src/jump_table/bitwise.rs'),
    "Neo.VM/JumpTable/JumpTable.Compound.cs": Path('crates/vm/src/jump_table/compound.rs'),
    "Neo.VM/JumpTable/JumpTable.Control.cs": Path('crates/vm/src/jump_table/control_ops.rs'),
    "Neo.VM/JumpTable/JumpTable.Numeric.cs": Path('crates/vm/src/jump_table/numeric.rs'),
    "Neo.VM/JumpTable/JumpTable.Push.cs": Path('crates/vm/src/jump_table/push.rs'),
    "Neo.VM/JumpTable/JumpTable.Slot.cs": Path('crates/vm/src/jump_table/slot.rs'),
    "Neo.VM/JumpTable/JumpTable.Splice.cs": Path('crates/vm/src/jump_table/splice.rs'),
    "Neo.VM/JumpTable/JumpTable.Stack.cs": Path('crates/vm/src/jump_table/stack.rs'),
    "Neo.VM/JumpTable/JumpTable.Types.cs": Path('crates/vm/src/jump_table/types.rs'),
    "Neo.VM/JumpTable/JumpTable.cs": Path('crates/vm/src/jump_table/jump_table.rs'),
    "Neo.VM/OpCode.cs": Path('crates/vm/src/op_code/op_code.rs'),
    "Neo.VM/OperandSizeAttribute.cs": Path('crates/vm/src/op_code/operand_size.rs'),
    "Neo.VM/ExceptionHandlingContext.cs": Path('crates/vm/src/exception_handling_context.rs'),
    "Neo.VM/ExceptionHandlingState.cs": Path('crates/vm/src/exception_handling_state.rs'),
    "Neo.VM/Types/Array.cs": Path('crates/vm/src/stack_item/array.rs'),
    "Neo.VM/Types/Boolean.cs": Path('crates/vm/src/stack_item/boolean.rs'),
    "Neo.VM/Types/Buffer.cs": Path('crates/vm/src/stack_item/buffer.rs'),
    "Neo.VM/Types/ByteString.cs": Path('crates/vm/src/stack_item/byte_string.rs'),
    "Neo.VM/Types/CompoundType.cs": Path('crates/vm/src/stack_item/compound_type.rs'),
    "Neo.VM/Types/Integer.cs": Path('crates/vm/src/stack_item/integer.rs'),
    "Neo.VM/Types/InteropInterface.cs": Path('crates/vm/src/stack_item/interop_interface.rs'),
    "Neo.VM/Types/Map.cs": Path('crates/vm/src/stack_item/map.rs'),
    "Neo.VM/Types/Null.cs": Path('crates/vm/src/stack_item/null.rs'),
    "Neo.VM/Types/Pointer.cs": Path('crates/vm/src/stack_item/pointer.rs'),
    "Neo.VM/Types/PrimitiveType.cs": Path('crates/vm/src/stack_item/primitive_type.rs'),
    "Neo.VM/Types/StackItem.Vertex.cs": Path('crates/vm/src/stack_item/stack_item_vertex.rs'),
    "Neo.VM/Types/StackItem.cs": Path('crates/vm/src/stack_item/stack_item.rs'),
    "Neo.VM/Types/StackItemType.cs": Path('crates/vm/src/stack_item/stack_item_type.rs'),
    "Neo.VM/Types/Struct.cs": Path('crates/vm/src/stack_item/struct_item.rs'),
    "Neo.VM/Collections/OrderedDictionary.cs": Path('crates/vm/src/collections/ordered_dictionary.rs'),
    "Neo.VM/VMUnhandledException.cs": Path('crates/vm/src/vm_unhandled_exception.rs'),
    "Neo/Extensions/ByteExtensions.cs": Path('crates/extensions/src/neo_byte_extensions.rs'),
    "Neo/Network/P2P/Payloads/ExtensiblePayload.cs": Path('crates/network/src/messages/extensible_payload.rs'),
    "Neo/Network/P2P/Payloads/Header.cs": Path('crates/network/src/messages/header.rs'),
    "Neo/Network/P2P/Payloads/IInventory.cs": Path('crates/network/src/payloads/iinventory.rs'),
    "Neo/Network/P2P/Payloads/IVerifiable.cs": Path('crates/network/src/payloads/iverifiable.rs'),
    "Neo/Network/P2P/Payloads/VersionPayload.cs": Path('crates/network/src/messages/version_payload.rs'),
    "Neo/Network/P2P/Capabilities/ArchivalNodeCapability.cs": Path('crates/network/src/messages/capabilities/archival_node_capability.rs'),
    "Neo/Network/P2P/Capabilities/DisableCompressionCapability.cs": Path('crates/network/src/messages/capabilities/disable_compression_capability.rs'),
    "Neo/Network/P2P/Capabilities/FullNodeCapability.cs": Path('crates/network/src/messages/capabilities/full_node_capability.rs'),
    "Neo/Network/P2P/Capabilities/NodeCapability.cs": Path('crates/network/src/messages/capabilities/node_capability.rs'),
    "Neo/Network/P2P/Capabilities/NodeCapabilityType.cs": Path('crates/network/src/messages/capabilities/node_capability_type.rs'),
    "Neo/Network/P2P/Capabilities/ServerCapability.cs": Path('crates/network/src/messages/capabilities/server_capability.rs'),
    "Neo/Network/P2P/Capabilities/UnknownCapability.cs": Path('crates/network/src/messages/capabilities/unknown_capability.rs'),
    "Neo/Network/P2P/Message.cs": Path('crates/network/src/messages/message.rs'),
    "Neo/Network/P2P/MessageCommand.cs": Path('crates/network/src/messages/message_command.rs'),
    "Neo/Network/P2P/MessageFlags.cs": Path('crates/network/src/messages/message_flags.rs'),
    "Neo/SmartContract/ApplicationEngine.cs": Path('crates/smart_contract/src/application_engine/application_engine.rs'),
} 

RUST_SKIP_NAMES = {"lib.rs", "mod.rs"}

SPECIAL_PREFIXES = {
    "UInt160": "uint160",
    "UInt256": "uint256",
    "NEP11": "nep11",
    "NEP17": "nep17",
    "NEP6": "nep6",
    "DBFT": "dbft",
    "RPC": "rpc",
}


def split_special(token: str) -> List[str]:
    for prefix, replacement in SPECIAL_PREFIXES.items():
        if token.startswith(prefix):
            remainder = token[len(prefix) :]
            if remainder:
                return [f"{replacement}_{camel_to_snake(remainder)}"]
            return [replacement]
    if token.upper() == token and len(token) > 1:
        return [token.lower()]
    return [camel_to_snake(token)]


def segment_to_parts(segment: str) -> List[str]:
    """Convert a C# path segment to one or more Rust path parts."""
    segment = segment.replace("-", "_").replace(".", "_")
    raw_parts = [p for p in re.split(r"[./]", segment) if p]
    parts: List[str] = []
    for raw in raw_parts:
        parts.extend(split_special(raw))
    return [p for p in parts if p]


def camel_to_snake(name: str) -> str:
    name = name.replace("-", "_")
    name = re.sub(r"([A-Z]+)([A-Z][a-z])", r"\1_\2", name)
    name = re.sub(r"([a-z\d])([A-Z])", r"\1_\2", name)
    name = re.sub(r"__+", "_", name)
    return name.lower()


def find_rule(rel_path: str) -> Optional[MappingRule]:
    for rule in MAPPING_RULES:
        if rule.matches(rel_path):
            return rule
    return None


def locate_rust_file(rel_path: str) -> Optional[Path]:
    if rel_path in FILE_OVERRIDES:
        return FILE_OVERRIDES[rel_path]

    rule = find_rule(rel_path)
    if not rule:
        return None

    tail = rel_path[len(rule.csharp_prefix) :].lstrip("/")
    if not tail:
        # Direct mapping (unlikely, but keep for completeness)
        return None

    *dir_segments, file_segment = tail.split("/")

    projected_dir = Path(rule.rust_prefix)
    for segment in dir_segments:
        for part in segment_to_parts(segment):
            projected_dir /= part

    if not file_segment.endswith(".cs"):
        return None

    stem = file_segment[:-3]  # strip .cs
    parts = segment_to_parts(stem)
    if not parts:
        return None

    *subdirs, filename = parts
    for subdir in subdirs:
        projected_dir /= subdir

    rust_file = projected_dir / f"{filename}.rs"
    return rust_file


def rust_file_exists(path: Optional[Path]) -> bool:
    if path is None:
        return False
    if path.name in RUST_SKIP_NAMES:
        return False
    candidate = (REPO_ROOT / path).resolve()
    return candidate.exists()


def generate_report() -> Dict[str, Any]:
    missing: Dict[str, List[str]] = {}
    matched = 0
    skipped = 0
    for cs_path in sorted(C_SHARP_ROOT.rglob("*.cs")):
        rel = cs_path.relative_to(C_SHARP_ROOT)
        rel_str = rel.as_posix()
        if rel_str in C_SHARP_EXCLUDES:
            skipped += 1
            continue
        if "/obj/" in rel_str or "/bin/" in rel_str:
            skipped += 1
            continue

        rust_path = locate_rust_file(rel_str)
        if rust_path and rust_file_exists(rust_path):
            matched += 1
            continue

        rule = find_rule(rel_str)
        key = rule.csharp_prefix if rule else "<no-rule>"
        missing.setdefault(key, []).append(rel_str)

    missing_groups = {
        group: sorted(entries)
        for group, entries in sorted(missing.items(), key=lambda item: item[0])
    }

    return {
        "matched": matched,
        "skipped": skipped,
        "missing_total": sum(len(entries) for entries in missing_groups.values()),
        "missing_groups": missing_groups,
    }


def render_markdown(report: Dict[str, Any], top_n: int = 25) -> str:
    lines: List[str] = []
    lines.append("# Neo Port Parity Report")
    lines.append("")
    lines.append(f"- Matched Rust files: {report['matched']}")
    lines.append(f"- Skipped C# files (excluded metadata): {report['skipped']}")
    lines.append(f"- Missing C# files without Rust counterpart: {report['missing_total']}")
    lines.append("")

    sorted_groups = sorted(
        report["missing_groups"].items(), key=lambda item: len(item[1]), reverse=True
    )

    lines.append("## Missing Coverage by Module")
    lines.append("")
    lines.append("| Module prefix | Missing files |")
    lines.append("| ------------- | ------------- |")
    for group, entries in sorted_groups:
        lines.append(f"| {group} | {len(entries)} |")
    lines.append("")

    lines.append("## Detailed Missing Files by Module")
    lines.append("")
    for group, entries in sorted_groups:
        lines.append(f"### {group}")
        for entry in entries:
            lines.append(f"- {entry}")
        lines.append("")

    return "\n".join(lines)


def main() -> None:
    parser = argparse.ArgumentParser(description="Check C# -> Rust port coverage")
    parser.add_argument("--json", type=Path, help="Write JSON report to this file")
    parser.add_argument(
        "--markdown", type=Path, help="Write Markdown summary to this file"
    )
    parser.add_argument(
        "--print", action="store_true", help="Emit JSON report to stdout"
    )
    args = parser.parse_args()

    report = generate_report()

    if args.json:
        args.json.parent.mkdir(parents=True, exist_ok=True)
        args.json.write_text(json.dumps(report, indent=2) + "\n")

    if args.markdown:
        args.markdown.parent.mkdir(parents=True, exist_ok=True)
        args.markdown.write_text(render_markdown(report) + "\n")

    if args.print or (not args.json and not args.markdown):
        print(json.dumps(report, indent=2))


if __name__ == "__main__":
    main()
