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
