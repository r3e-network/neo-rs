#!/usr/bin/env python3
"""Extract per-window MainNet import profiling from structured node logs."""

from __future__ import annotations

import json
from pathlib import Path
from typing import Any, Iterable, Iterator


PROGRESS_MESSAGE = "chain.acc import progress"
COMPLETE_MESSAGE = "chain.acc import complete"
IMPORT_START_MESSAGE = "importing blocks from chain.acc"
MPT_MUTATION_MESSAGE = "chain.acc MPT mutation profile"
MDBX_VALUE_SIZE_MESSAGE = "chain.acc MDBX value-size profile"
MDBX_CURSOR_RESOLVE_MESSAGE = "chain.acc MDBX cursor-resolve profile"
VM_EXECUTION_PROFILE_MESSAGE = "targeted VM execution profile"
SAMPLE_EXTENSION_MESSAGES = {
    MPT_MUTATION_MESSAGE,
    MDBX_VALUE_SIZE_MESSAGE,
    MDBX_CURSOR_RESOLVE_MESSAGE,
}
COUNT_FIELDS = (
    "imported",
    "transaction_blocks",
    "transactions",
    "empty_blocks",
    "empty_only_blocks",
)
ELAPSED_FIELDS = (
    "elapsed_seconds",
    "transaction_block_import_seconds",
    "empty_block_import_seconds",
)
HOTSPOT_PREFIXES = (
    "mdbx_commit_",
    "native_",
    "neotoken_",
    "state_service_mpt_",
)
HOTSPOT_LABEL_SUFFIXES = (
    "_stage",
    "_trigger",
    "_contract",
    "_contract_id",
    "_kind",
)
NON_EXECUTION_TIMING_PARTS = (
    "_end_to_end_",
    "_queue_wait_",
    "_enqueue_blocking_",
)
NON_EXECUTION_TIMING_NAMES = {
    "state_service_mpt_avg_total_us",
}
HOTSPOT_LIMIT = 8
# A 10,000-block profile can contain one unique transaction entry script per
# transaction in addition to stable contract bytecode. Keep enough bounded
# offline capacity to avoid first-seen entry scripts hiding later contracts.
VM_SCRIPT_CAPACITY = 16_384
VM_ENTRY_CAPACITY = 128
VM_LOGICAL_CONTEXT_CAPACITY = 256
VM_OUTPUT_LIMIT = 128


def structured_log_fields(lines: Iterable[str]) -> Iterator[dict[str, Any]]:
    """Yield structured tracing fields while ignoring unrelated log lines."""
    for line in lines:
        try:
            payload = json.loads(line)
        except (json.JSONDecodeError, TypeError):
            continue
        if not isinstance(payload, dict):
            continue
        fields = payload.get("fields")
        if isinstance(fields, dict):
            yield fields


def parse_chain_acc_import_profile(text: str) -> dict[str, Any]:
    """Parse the latest chain.acc attempt and derive non-cumulative windows."""
    return parse_chain_acc_import_lines(text.splitlines())


def parse_chain_acc_import_lines(lines: Iterable[str]) -> dict[str, Any]:
    """Parse structured log lines without retaining the full replay output."""
    active_samples: list[dict[str, Any]] = []
    latest_samples: list[dict[str, Any]] = []
    latest_complete: dict[str, Any] | None = None
    active_vm_profile = new_vm_script_profile()
    latest_vm_profile = new_vm_script_profile()
    active_import = False

    for fields in structured_log_fields(lines):
        message = fields.get("message")
        if message == IMPORT_START_MESSAGE:
            active_samples = []
            latest_samples = []
            latest_complete = None
            active_vm_profile = new_vm_script_profile()
            latest_vm_profile = new_vm_script_profile()
            active_import = True
        elif message == VM_EXECUTION_PROFILE_MESSAGE:
            aggregate_vm_execution_profile(active_vm_profile, fields)
        elif message == PROGRESS_MESSAGE:
            active_import = True
            sample = {key: value for key, value in fields.items() if key != "message"}
            if active_samples and cumulative_progress_reset(active_samples[-1], sample):
                active_samples = []
            active_samples.append(sample)
            latest_complete = None
        elif message in SAMPLE_EXTENSION_MESSAGES and active_samples:
            sample = {key: value for key, value in fields.items() if key != "message"}
            if sample.get("imported") == active_samples[-1].get("imported"):
                active_samples[-1].update(sample)
        elif message == COMPLETE_MESSAGE:
            latest_complete = {
                key: value for key, value in fields.items() if key != "message"
            }
            latest_samples = active_samples
            latest_vm_profile = active_vm_profile
            active_samples = []
            active_vm_profile = new_vm_script_profile()
            active_import = False

    if active_import or active_samples or active_vm_profile["transaction_count"]:
        latest_samples = active_samples
        latest_complete = None
        latest_vm_profile = active_vm_profile

    windows = derive_profile_windows(latest_samples)
    return {
        "import_report": latest_complete,
        "profile_windows": windows,
        "profile_hotspots": summarize_profile_hotspots(windows),
        "vm_script_profile": finish_vm_script_profile(latest_vm_profile),
    }


def new_vm_script_profile() -> dict[str, Any]:
    """Create bounded mutable state for one import attempt's VM profiles."""
    return {
        "transaction_count": 0,
        "execute_us_total": 0,
        "profiled_instructions_total": 0,
        "collector_overflow_instructions": 0,
        "collector_overflow_context_loads": 0,
        "unreported_retained_script_instructions": 0,
        "malformed_script_records": 0,
        "malformed_application_context_profiles": 0,
        "application_context_other_loads": 0,
        "logical_context_overflow_loads": 0,
        "script_capacity": VM_SCRIPT_CAPACITY,
        "script_overflow_records": 0,
        "script_overflow_instructions": 0,
        "script_overflow_context_loads": 0,
        "protocols": {},
        "network_magics": {},
        "hardfork_contexts": {},
        "scripts": {},
    }


def aggregate_vm_execution_profile(
    aggregate: dict[str, Any], fields: dict[str, Any]
) -> None:
    """Merge one bounded per-transaction VM profile into replay evidence."""
    aggregate["transaction_count"] += 1
    execute_us = nonnegative_integer(fields.get("execute_us"))
    profiled_instructions = nonnegative_integer(fields.get("profiled_instructions"))
    collector_overflow_instructions = nonnegative_integer(
        fields.get("other_script_instructions")
    )
    aggregate["execute_us_total"] += execute_us
    aggregate["profiled_instructions_total"] += profiled_instructions
    aggregate["collector_overflow_instructions"] += collector_overflow_instructions
    aggregate["collector_overflow_context_loads"] += nonnegative_integer(
        fields.get("other_script_context_loads")
    )
    count_bounded_label(aggregate["protocols"], fields.get("protocol"))
    count_bounded_label(aggregate["network_magics"], fields.get("network_magic"))
    count_bounded_label(
        aggregate["hardfork_contexts"], fields.get("hardfork_context")
    )

    parsed_scripts, malformed = parse_vm_hottest_scripts(fields.get("hottest_scripts"))
    aggregate["malformed_script_records"] += malformed
    reported_instructions = 0
    for script in parsed_scripts:
        reported_instructions += script["instructions"]
        key = (script["script_hash"], script["script_bytes"])
        current = aggregate["scripts"].get(key)
        if current is None:
            if len(aggregate["scripts"]) >= aggregate["script_capacity"]:
                aggregate["script_overflow_records"] += 1
                aggregate["script_overflow_instructions"] += script["instructions"]
                aggregate["script_overflow_context_loads"] += script["contexts"]
                continue
            current = {
                "script_hash": script["script_hash"],
                "script_bytes": script["script_bytes"],
                "instructions": 0,
                "contexts": 0,
                "transaction_appearances": 0,
                "inclusive_execute_us": 0,
                "other_entry_context_loads": 0,
                "entry_context_loads": {},
                "entry_overflow_context_loads": 0,
                "logical_contexts": {},
                "logical_context_overflow_loads": 0,
            }
            aggregate["scripts"][key] = current
        current["instructions"] += script["instructions"]
        current["contexts"] += script["contexts"]
        current["transaction_appearances"] += 1
        current["inclusive_execute_us"] += execute_us
        current["other_entry_context_loads"] += script[
            "other_entry_context_loads"
        ]
        for entry_offset, context_loads in script["entry_context_loads"].items():
            entries = current["entry_context_loads"]
            if entry_offset in entries:
                entries[entry_offset] += context_loads
            elif len(entries) < VM_ENTRY_CAPACITY:
                entries[entry_offset] = context_loads
            else:
                current["entry_overflow_context_loads"] += context_loads

    contexts, malformed_contexts, other_context_loads = parse_application_contexts(
        fields.get("application_contexts")
    )
    aggregate["malformed_application_context_profiles"] += malformed_contexts
    aggregate["application_context_other_loads"] += other_context_loads
    for context in contexts:
        key = (context["raw_script_hash"], context["raw_script_bytes"])
        current = aggregate["scripts"].get(key)
        if current is None:
            if len(aggregate["scripts"]) >= aggregate["script_capacity"]:
                aggregate["logical_context_overflow_loads"] += context["context_loads"]
                continue
            current = {
                "script_hash": context["raw_script_hash"],
                "script_bytes": context["raw_script_bytes"],
                "instructions": 0,
                "contexts": 0,
                "transaction_appearances": 0,
                "inclusive_execute_us": 0,
                "other_entry_context_loads": 0,
                "entry_context_loads": {},
                "entry_overflow_context_loads": 0,
                "logical_contexts": {},
                "logical_context_overflow_loads": 0,
            }
            aggregate["scripts"][key] = current
        identity = tuple(
            context.get(name)
            for name in (
                "entry_offset",
                "logical_script_hash",
                "contract_id",
                "contract_update_counter",
                "nef_checksum",
                "manifest_name",
                "method",
                "argument_count",
                "return_type",
                "call_flags",
                "dynamic_call",
            )
        ) + (tuple(context["parameter_types"]),)
        logical_contexts = current["logical_contexts"]
        if identity in logical_contexts:
            logical_contexts[identity]["context_loads"] += context["context_loads"]
        elif len(logical_contexts) < VM_LOGICAL_CONTEXT_CAPACITY:
            logical_contexts[identity] = {
                name: value
                for name, value in context.items()
                if name not in {"raw_script_hash", "raw_script_bytes"}
            }
        else:
            current["logical_context_overflow_loads"] += context["context_loads"]

    aggregate["unreported_retained_script_instructions"] += max(
        profiled_instructions
        - collector_overflow_instructions
        - reported_instructions,
        0,
    )


def parse_vm_hottest_scripts(raw: Any) -> tuple[list[dict[str, Any]], int]:
    """Decode the stable bounded script summary emitted by the Rust node."""
    if not isinstance(raw, str) or not raw:
        return [], 0
    parsed: list[dict[str, Any]] = []
    malformed = 0
    for encoded in raw.split(";"):
        parts = encoded.split(":")
        attributes: dict[str, str] = {}
        for part in parts[1:]:
            if "=" in part:
                name, value = part.split("=", 1)
                attributes[name] = value
        try:
            script_hash = parts[0]
            if not script_hash.startswith("0x") or len(script_hash) != 42:
                raise ValueError("invalid UInt160")
            script_bytes = parse_nonnegative_integer(attributes["bytes"])
            instructions = parse_nonnegative_integer(attributes["instructions"])
            contexts = parse_nonnegative_integer(attributes["contexts"])
            other_entries = parse_nonnegative_integer(attributes["other_entries"])
            entries: dict[int, int] = {}
            if attributes.get("entries"):
                for entry in attributes["entries"].split("+"):
                    offset, count = entry.rsplit("x", 1)
                    entry_offset = parse_nonnegative_integer(offset)
                    entries[entry_offset] = entries.get(entry_offset, 0) + parse_nonnegative_integer(
                        count
                    )
            parsed.append(
                {
                    "script_hash": script_hash,
                    "script_bytes": script_bytes,
                    "instructions": instructions,
                    "contexts": contexts,
                    "other_entry_context_loads": other_entries,
                    "entry_context_loads": entries,
                }
            )
        except (KeyError, ValueError):
            malformed += 1
    return parsed, malformed


def parse_application_contexts(raw: Any) -> tuple[list[dict[str, Any]], int, int]:
    """Decode structured logical contract/method identities from one transaction."""
    if raw in (None, ""):
        return [], 0, 0
    try:
        payload = json.loads(raw) if isinstance(raw, str) else raw
        if not isinstance(payload, dict) or not isinstance(payload.get("contexts"), list):
            raise ValueError("invalid application context profile")
        other_context_loads = parse_nonnegative_integer(
            payload.get("other_context_loads", 0)
        )
    except (TypeError, ValueError, json.JSONDecodeError):
        return [], 1, 0

    parsed: list[dict[str, Any]] = []
    malformed = 0
    for context in payload["contexts"]:
        try:
            if not isinstance(context, dict):
                raise ValueError("context must be an object")
            raw_script_hash = str(context["raw_script_hash"])
            logical_script_hash = str(context["logical_script_hash"])
            if (
                not raw_script_hash.startswith("0x")
                or len(raw_script_hash) != 42
                or not logical_script_hash.startswith("0x")
                or len(logical_script_hash) != 42
            ):
                raise ValueError("invalid UInt160")
            parameter_types = context.get("parameter_types", [])
            if not isinstance(parameter_types, list):
                raise ValueError("parameter_types must be an array")
            parsed.append(
                {
                    "raw_script_hash": raw_script_hash,
                    "raw_script_bytes": parse_nonnegative_integer(
                        context["raw_script_bytes"]
                    ),
                    "entry_offset": parse_nonnegative_integer(context["entry_offset"]),
                    "logical_script_hash": logical_script_hash,
                    "contract_id": optional_integer(context.get("contract_id")),
                    "contract_update_counter": optional_nonnegative_integer(
                        context.get("contract_update_counter")
                    ),
                    "nef_checksum": optional_nonnegative_integer(
                        context.get("nef_checksum")
                    ),
                    "manifest_name": optional_string(context.get("manifest_name")),
                    "method": optional_string(context.get("method")),
                    "argument_count": parse_nonnegative_integer(
                        context.get("argument_count", 0)
                    ),
                    "parameter_types": [str(value) for value in parameter_types],
                    "return_type": optional_string(context.get("return_type")),
                    "call_flags": parse_nonnegative_integer(
                        context.get("call_flags", 0)
                    ),
                    "dynamic_call": bool(context.get("dynamic_call", False)),
                    "context_loads": parse_nonnegative_integer(
                        context.get("context_loads", 0)
                    ),
                }
            )
        except (KeyError, TypeError, ValueError):
            malformed += 1
    return parsed, malformed, other_context_loads


def finish_vm_script_profile(aggregate: dict[str, Any]) -> dict[str, Any]:
    """Produce a deterministic bounded JSON-ready script ranking."""
    total_instructions = aggregate["profiled_instructions_total"]
    scripts = []
    for script in aggregate["scripts"].values():
        entries = [
            {"entry_offset": offset, "context_loads": count}
            for offset, count in sorted(
                script["entry_context_loads"].items(),
                key=lambda item: (-item[1], item[0]),
            )
        ]
        logical_contexts = sorted(
            script["logical_contexts"].values(),
            key=lambda context: (
                -context["context_loads"],
                context["logical_script_hash"],
                context["entry_offset"],
                context.get("method") or "",
            ),
        )
        scripts.append(
            {
                key: value
                for key, value in script.items()
                if key not in {"entry_context_loads", "logical_contexts"}
            }
            | {
                "instruction_share": ratio(script["instructions"], total_instructions),
                "entry_points": entries,
                "logical_contexts": logical_contexts,
            }
        )
    scripts.sort(
        key=lambda script: (
            -script["instructions"],
            script["script_hash"],
            script["script_bytes"],
        )
    )
    output = {
        key: value
        for key, value in aggregate.items()
        if key not in {"scripts", "protocols", "network_magics", "hardfork_contexts"}
    }
    output.update(
        {
            "profiled_execute_seconds": rounded(aggregate["execute_us_total"] / 1_000_000),
            "protocols": finish_bounded_labels(aggregate["protocols"]),
            "network_magics": finish_bounded_labels(aggregate["network_magics"]),
            "hardfork_contexts": finish_bounded_labels(
                aggregate["hardfork_contexts"]
            ),
            "ranked_script_count": len(scripts),
            "reported_script_limit": VM_OUTPUT_LIMIT,
            "scripts": scripts[:VM_OUTPUT_LIMIT],
        }
    )
    return output


def count_bounded_label(counts: dict[str, int], value: Any) -> None:
    if value is None:
        return
    label = str(value)
    if label in counts:
        counts[label] += 1
    elif len(counts) < 64:
        counts[label] = 1


def finish_bounded_labels(counts: dict[str, int]) -> list[dict[str, Any]]:
    return [
        {"value": value, "count": count}
        for value, count in sorted(counts.items(), key=lambda item: (-item[1], item[0]))
    ]


def parse_nonnegative_integer(value: Any) -> int:
    parsed = int(value)
    if parsed < 0:
        raise ValueError("negative integer")
    return parsed


def optional_integer(value: Any) -> int | None:
    if value is None:
        return None
    return int(value)


def optional_nonnegative_integer(value: Any) -> int | None:
    if value is None:
        return None
    return parse_nonnegative_integer(value)


def optional_string(value: Any) -> str | None:
    if value is None:
        return None
    return str(value)


def nonnegative_integer(value: Any) -> int:
    try:
        return parse_nonnegative_integer(value)
    except (TypeError, ValueError):
        return 0


def ratio(numerator: int, denominator: int) -> float:
    if denominator <= 0:
        return 0.0
    return rounded(numerator / denominator)


def read_chain_acc_import_profile(path: Path | None) -> dict[str, Any]:
    """Read a bounded replay log and return its latest import profile."""
    if path is None or not path.is_file():
        return parse_chain_acc_import_profile("")
    with path.open("r", encoding="utf-8", errors="replace") as lines:
        return parse_chain_acc_import_lines(lines)


def read_chain_acc_import_report(path: Path | None) -> dict[str, Any] | None:
    """Return the latest completion record for compatibility with old callers."""
    return read_chain_acc_import_profile(path)["import_report"]


def cumulative_progress_reset(
    previous: dict[str, Any], current: dict[str, Any]
) -> bool:
    """Detect a new appended import attempt without joining its counters."""
    for field in ("imported", "elapsed_seconds"):
        previous_value = numeric_value(previous.get(field))
        current_value = numeric_value(current.get(field))
        if (
            previous_value is not None
            and current_value is not None
            and current_value < previous_value
        ):
            return True
    return False


def derive_profile_windows(samples: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Turn cumulative progress samples into ordered, independent windows."""
    previous_counts = {field: 0 for field in COUNT_FIELDS}
    previous_elapsed = {field: 0.0 for field in ELAPSED_FIELDS}
    windows: list[dict[str, Any]] = []

    for sample in samples:
        counts = {
            field: integer_value(sample.get(field), previous_counts[field])
            for field in COUNT_FIELDS
        }
        elapsed = {
            field: float_value(sample.get(field), previous_elapsed[field])
            for field in ELAPSED_FIELDS
        }
        count_deltas = {
            field: max(counts[field] - previous_counts[field], 0)
            for field in COUNT_FIELDS
        }
        elapsed_deltas = {
            field: max(elapsed[field] - previous_elapsed[field], 0.0)
            for field in ELAPSED_FIELDS
        }
        window = {
            "index": len(windows) + 1,
            "from_imported": previous_counts["imported"],
            "to_imported": counts["imported"],
            **count_deltas,
            **{
                field: rounded(value)
                for field, value in elapsed_deltas.items()
            },
            "blocks_per_second": rate(
                count_deltas["imported"], elapsed_deltas["elapsed_seconds"]
            ),
            "transaction_blocks_per_second": rate(
                count_deltas["transaction_blocks"],
                elapsed_deltas["transaction_block_import_seconds"],
            ),
            "empty_blocks_per_second": rate(
                count_deltas["empty_blocks"],
                elapsed_deltas["empty_block_import_seconds"],
            ),
            "hotspots": extract_hotspots(sample),
        }
        windows.append(window)
        previous_counts = counts
        previous_elapsed = elapsed

    return windows


def extract_hotspots(sample: dict[str, Any]) -> dict[str, Any]:
    """Keep stable native-contract and MPT profiling fields."""
    return {
        key: sample[key]
        for key in sorted(sample)
        if key.startswith(HOTSPOT_PREFIXES) and sample[key] is not None
    }


def summarize_profile_hotspots(windows: list[dict[str, Any]]) -> dict[str, Any]:
    """Build a compact rate and native/MPT hotspot summary."""
    if not windows:
        return {
            "window_count": 0,
            "slowest_window": None,
            "fastest_window": None,
            "top_native_mpt_by_max_us": [],
            "latest_labels": {},
        }

    slowest = min(windows, key=lambda item: (item["blocks_per_second"], item["index"]))
    fastest = max(windows, key=lambda item: (item["blocks_per_second"], -item["index"]))
    timings: dict[str, dict[str, Any]] = {}
    for window in windows:
        for name, value in window.get("hotspots", {}).items():
            if not rankable_execution_timing(name):
                continue
            timing = numeric_value(value)
            if timing is None:
                continue
            current = timings.setdefault(
                name,
                {
                    "name": name,
                    "max_us": timing,
                    "last_us": timing,
                    "window_index": window["index"],
                },
            )
            current["last_us"] = timing
            if timing > current["max_us"]:
                current["max_us"] = timing
                current["window_index"] = window["index"]

    latest_hotspots = windows[-1].get("hotspots", {})
    latest_labels = {
        name: value
        for name, value in latest_hotspots.items()
        if name.endswith(HOTSPOT_LABEL_SUFFIXES) and value not in (None, "")
    }
    top_timings = sorted(
        (timing for timing in timings.values() if timing["max_us"] > 0.0),
        key=lambda item: (-item["max_us"], item["name"]),
    )[:HOTSPOT_LIMIT]
    return {
        "window_count": len(windows),
        "slowest_window": compact_window(slowest),
        "fastest_window": compact_window(fastest),
        "top_native_mpt_by_max_us": top_timings,
        "latest_labels": latest_labels,
    }


def rankable_execution_timing(name: str) -> bool:
    """Select comparable per-sample work timings, excluding queue latency."""
    if not name.endswith("_us") or "_avg_" not in name:
        return False
    if name in NON_EXECUTION_TIMING_NAMES:
        return False
    return not any(part in name for part in NON_EXECUTION_TIMING_PARTS)


def compact_window(window: dict[str, Any]) -> dict[str, Any]:
    return {
        "index": window["index"],
        "from_imported": window["from_imported"],
        "to_imported": window["to_imported"],
        "blocks_per_second": window["blocks_per_second"],
        "transaction_blocks_per_second": window["transaction_blocks_per_second"],
        "empty_blocks_per_second": window["empty_blocks_per_second"],
    }


def integer_value(value: Any, default: int) -> int:
    try:
        return int(value)
    except (TypeError, ValueError):
        return default


def float_value(value: Any, default: float) -> float:
    parsed = numeric_value(value)
    return default if parsed is None else parsed


def numeric_value(value: Any) -> float | None:
    if isinstance(value, bool):
        return None
    try:
        return float(value)
    except (TypeError, ValueError):
        return None


def rounded(value: float) -> float:
    return round(value, 6)


def rate(count: int, elapsed_seconds: float) -> float:
    if elapsed_seconds <= 0.0:
        return 0.0
    return rounded(count / elapsed_seconds)
