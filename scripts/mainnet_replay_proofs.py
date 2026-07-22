"""Proof-accounting helpers shared by bounded MainNet replay workflows."""

from __future__ import annotations

from typing import Any


def transaction_work_summary_from_fast_sync_report(
    fast_sync_report: dict[str, Any],
) -> dict[str, Any]:
    """Project native transaction execution evidence from a fast-sync sidecar."""
    import_report = fast_sync_report.get("import")
    hot_metrics = fast_sync_report.get("hot_metrics")
    if not isinstance(import_report, dict):
        import_report = {}
    if not isinstance(hot_metrics, dict):
        hot_metrics = {}

    try:
        transaction_blocks = int(import_report.get("transaction_blocks") or 0)
        transactions = int(import_report.get("transactions") or 0)
        transaction_bps = float(
            import_report.get("transaction_blocks_per_second") or 0.0
        )
    except (TypeError, ValueError):
        transaction_blocks = 0
        transactions = 0
        transaction_bps = 0.0

    stage_metrics: list[dict[str, Any]] = []
    execution_stage_calls = 0
    stage_source = "fast-sync-native-tx-stages"
    native_persist_tx = import_report.get("native_persist_tx")
    stages = None
    if isinstance(native_persist_tx, dict):
        stages = native_persist_tx.get("stages")
        if isinstance(stages, list):
            stage_source = "fast-sync-import-native-tx-stages"
    if not isinstance(stages, list):
        stages = hot_metrics.get("native_persist_tx_stages")
    if isinstance(stages, list):
        for stage in stages:
            if not isinstance(stage, dict):
                continue
            try:
                stage_name = str(stage["stage"])
                calls = int(stage["calls"])
                total_us = int(stage.get("total_us") or 0)
                average_us = int(stage.get("avg_us") or 0)
            except (KeyError, TypeError, ValueError):
                continue
            if not stage_name or calls < 0 or total_us < 0:
                continue
            if stage_name == "execute":
                execution_stage_calls = max(execution_stage_calls, calls)
            stage_metrics.append(
                {
                    "name": f"native_persist_tx_stage:{stage_name}",
                    "stage": stage_name,
                    "sample_count": 1,
                    "average": calls,
                    "last": calls,
                    "max": calls,
                    "total_us": total_us,
                    "average_us": average_us,
                    "observed_transaction_work": stage_name == "execute" and calls > 0,
                }
            )

    observed_transaction_work = (
        transaction_blocks > 0
        and transactions > 0
        and execution_stage_calls >= transactions
    )
    return {
        "required_for_speed_proof": True,
        "observed_transaction_work": observed_transaction_work,
        "source": stage_source,
        "transaction_blocks": transaction_blocks,
        "transactions": transactions,
        "transaction_blocks_per_second": transaction_bps,
        "native_execution_stage_calls": execution_stage_calls,
        "metric_count": len(stage_metrics),
        "metrics": stage_metrics,
    }


def apply_import_speed_measurement(
    report: dict[str, Any], measurement: dict[str, Any] | None
) -> None:
    """Apply an authoritative import-speed measurement to a replay report."""
    if measurement is None:
        return

    measured_bps = float(measurement["blocks_per_second"])
    report["sync_speed_measurement_source"] = measurement["source"]
    report["sync_speed_measured_blocks_per_second"] = measured_bps

    try:
        floor_bps = float(report["sync_speed_floor_blocks_per_second"])
    except (KeyError, TypeError, ValueError):
        floor_bps = None
    if floor_bps is not None:
        shortfall = max(floor_bps - measured_bps, 0.0)
        report["sync_speed_shortfall_blocks_per_second"] = shortfall
        floor_met = shortfall == 0.0
        if shortfall > 0.0 and report.get("status") in {
            "target-reached",
            "sync-speed-too-slow",
            "transaction-work-unproven",
        }:
            report["status"] = "sync-speed-too-slow"
    else:
        floor_met = True

    try:
        ceiling_bps = float(report["sync_speed_ceiling_blocks_per_second"])
    except (KeyError, TypeError, ValueError):
        ceiling_bps = None
    if ceiling_bps is not None:
        overage = max(measured_bps - ceiling_bps, 0.0)
        report["sync_speed_overage_blocks_per_second"] = overage
        ceiling_met = overage == 0.0
        if overage > 0.0 and report.get("status") == "target-reached":
            report["status"] = "sync-speed-too-fast"
    else:
        ceiling_met = True

    if floor_bps is not None or ceiling_bps is not None:
        report["sync_speed_band_met"] = floor_met and ceiling_met


def chain_acc_import_speed_measurement(
    report: dict[str, Any],
) -> dict[str, Any] | None:
    """Select transaction-bearing chain.acc throughput when it is available."""
    if report.get("sync_source") != "import-chain":
        return None
    import_report = report.get("chain_acc_import_report")
    if not isinstance(import_report, dict):
        return None

    try:
        transaction_blocks = int(import_report.get("transaction_blocks") or 0)
        transaction_bps = float(import_report["transaction_blocks_per_second"])
    except (KeyError, TypeError, ValueError):
        transaction_blocks = 0
        transaction_bps = 0.0

    if transaction_blocks > 0:
        return {
            "source": "import-chain-transaction-blocks",
            "blocks_per_second": transaction_bps,
        }

    try:
        average_bps = float(import_report["average_blocks_per_second"])
    except (KeyError, TypeError, ValueError):
        return None
    return {
        "source": "import-chain",
        "blocks_per_second": average_bps,
    }


def apply_chain_acc_import_speed_measurement(report: dict[str, Any]) -> None:
    """Apply the best available chain.acc speed measurement to a report."""
    apply_import_speed_measurement(report, chain_acc_import_speed_measurement(report))


def chain_acc_import_satisfies_speed_gate(report: dict[str, Any]) -> bool:
    """Return whether chain.acc proves target coverage and transaction speed."""
    if report.get("sync_source") != "import-chain":
        return False
    import_report = report.get("chain_acc_import_report")
    if not isinstance(import_report, dict):
        return False

    try:
        transaction_blocks = int(import_report.get("transaction_blocks") or 0)
        transaction_bps = float(
            import_report.get("transaction_blocks_per_second") or 0.0
        )
        target_height = int(report.get("target_height"))
        floor = report.get("sync_speed_floor_blocks_per_second")
        floor_bps = float(floor) if floor is not None else None
    except (TypeError, ValueError):
        return False

    # The bounded runner records the initial visible height, so imports that
    # omit a redundant final_height can still prove their terminal height.
    try:
        final_height_value = import_report.get("final_height")
        if final_height_value is not None:
            final_height = int(final_height_value)
        else:
            imported = int(import_report.get("imported"))
            samples = report.get("height_samples") or []
            initial_height = next(
                int(sample["height"])
                for sample in samples
                if isinstance(sample, dict) and "height" in sample
            )
            final_height = initial_height + imported
    except (StopIteration, TypeError, ValueError):
        return False

    return (
        transaction_blocks > 0
        and final_height >= target_height
        and (floor_bps is None or transaction_bps >= floor_bps)
    )


def recover_chain_acc_status_from_import_proof(report: dict[str, Any]) -> None:
    """Recover a tentative replay status from authoritative chain.acc proof."""
    if report.get("status") not in {
        "metrics-unavailable",
        "sync-speed-too-slow",
        "transaction-work-unproven",
    }:
        return
    apply_chain_acc_import_speed_measurement(report)
    if chain_acc_import_satisfies_speed_gate(report):
        report["status"] = "target-reached"


def transaction_work_summary_from_chain_acc_import(
    import_report: dict[str, Any],
) -> dict[str, Any]:
    """Project transaction-bearing work evidence from a chain.acc report."""
    try:
        transaction_blocks = int(import_report.get("transaction_blocks") or 0)
        transactions = int(import_report.get("transactions") or 0)
        transaction_bps = float(
            import_report.get("transaction_blocks_per_second") or 0.0
        )
    except (TypeError, ValueError):
        transaction_blocks = 0
        transactions = 0
        transaction_bps = 0.0
    observed_transaction_work = transaction_blocks > 0 and transactions > 0
    return {
        "required_for_speed_proof": True,
        "observed_transaction_work": observed_transaction_work,
        "source": "chain-acc-import-log",
        "transaction_blocks": transaction_blocks,
        "transactions": transactions,
        "transaction_blocks_per_second": transaction_bps,
        "metric_count": 1,
        "metrics": [
            {
                "name": "chain_acc_import_transaction_blocks",
                "sample_count": 1,
                "average": transaction_blocks,
                "last": transaction_blocks,
                "max": transaction_blocks,
                "observed_transaction_work": observed_transaction_work,
            }
        ],
    }


def height_sample_rate_summary(report: dict[str, Any]) -> dict[str, Any]:
    """Summarize visible-height advances without shortening atomic plateaus."""
    samples = report.get("height_samples") or []
    intervals: list[dict[str, Any]] = []
    plateau_start: tuple[float, int] | None = None
    for sample in samples:
        if not isinstance(sample, dict):
            continue
        try:
            to_elapsed = float(sample.get("elapsed_seconds"))
            to_height = int(sample.get("height"))
        except (TypeError, ValueError):
            plateau_start = None
            continue
        if plateau_start is None:
            plateau_start = (to_elapsed, to_height)
            continue

        from_elapsed, from_height = plateau_start
        if to_height == from_height:
            continue
        elapsed_delta = to_elapsed - from_elapsed
        height_delta = to_height - from_height
        if elapsed_delta > 0 and height_delta > 0:
            intervals.append(
                {
                    "from_height": from_height,
                    "to_height": to_height,
                    "height_delta": height_delta,
                    "elapsed_seconds": elapsed_delta,
                    "blocks_per_second": height_delta / elapsed_delta,
                }
            )
        plateau_start = (to_elapsed, to_height)

    sample_count = len([sample for sample in samples if isinstance(sample, dict)])
    if not intervals:
        return {
            "sample_count": sample_count,
            "interval_count": 0,
            "average_blocks_per_second": 0.0,
            "min_blocks_per_second": 0.0,
            "max_blocks_per_second": 0.0,
            "slowest_interval": None,
            "fastest_interval": None,
        }

    rates = [float(interval["blocks_per_second"]) for interval in intervals]
    slowest = min(intervals, key=lambda item: float(item["blocks_per_second"]))
    fastest = max(intervals, key=lambda item: float(item["blocks_per_second"]))
    return {
        "sample_count": sample_count,
        "interval_count": len(intervals),
        "average_blocks_per_second": sum(rates) / len(rates),
        "min_blocks_per_second": min(rates),
        "max_blocks_per_second": max(rates),
        "slowest_interval": slowest,
        "fastest_interval": fastest,
    }
