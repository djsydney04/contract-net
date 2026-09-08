#!/usr/bin/env python3
"""
Benchmark this machine and (optionally) your custom execute().

Uses contractnet.benchmark for calibration rates, then times the same golden
workloads as verify.py so you can track speedups over time.

    python3 benchmark.py

Each run appends a timestamped block to benchmark_data/benchmarks.log.
verify.py stays for correctness only — use this when you care about speed.
"""

from __future__ import annotations

import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Mapping

from contractnet.benchmark import calibrate, overall_score, work_units
from contractnet.tasks import run_task

ROOT = Path(__file__).resolve().parent
BENCHMARK_DIR = ROOT / "benchmark_data"
BENCHMARK_LOG = BENCHMARK_DIR / "benchmarks.log"

# Same pinned workloads as verify.py — comparable run to run.
GOLDEN: list[tuple[str, Mapping[str, Any], int]] = [
    ("monte_carlo_pi", {"seed": 12345, "samples": 50000}, 39336),
    ("prime_count", {"lo": 1000000, "hi": 1010000}, 753),
    ("hash_search", {"seed": "cpsc370-golden", "threshold": 65536}, 90984),
    ("sort_checksum", {"seed": 99, "n": 50000}, 1785318802179667385),
    ("matmul_mod", {"seed": 2026, "n": 40, "mod": 1000003}, 629524),
]


def time_reference() -> list[str]:
    print("Timing reference implementations on golden workloads…")
    lines: list[str] = ["reference golden workloads:"]
    for task_type, params, _expected in GOLDEN:
        started = time.perf_counter()
        run_task(task_type, params)
        elapsed = time.perf_counter() - started
        units = work_units(task_type, params)
        rate = units / max(elapsed, 1e-6)
        line = f"  {task_type:<16} {elapsed:6.3f}s  {rate:,.0f} units/s"
        print(line)
        lines.append(line)
    return lines


def time_custom_execute() -> list[str]:
    try:
        from my_contractor import MyContractor
    except Exception as err:  # noqa: BLE001
        msg = f"Could not import MyContractor: {err!r}"
        print(f"\n{msg}")
        return [msg]

    from contractnet import Contractor, Task

    if MyContractor.execute is Contractor.execute:
        msg = "No custom execute(). Skipping custom timing."
        print(f"\n{msg}")
        return [msg]

    print("\nTiming your custom execute() vs reference…")
    agent = MyContractor.__new__(MyContractor)
    lines: list[str] = ["custom execute() vs reference:"]

    for task_type, params, expected in GOLDEN:
        task = Task(
            task_id=0,
            task_type=task_type,
            params=dict(params),
            budget=999.0,
            deadline_s=999.0,
            bid_window_ms=0,
            attempt=1,
        )

        started = time.perf_counter()
        reference = run_task(task_type, params)
        ref_time = time.perf_counter() - started

        started = time.perf_counter()
        yours = agent.execute(task)
        your_time = time.perf_counter() - started

        match = str(yours) == str(reference) == str(expected)
        speedup = ref_time / your_time if your_time > 0 else float("inf")
        mark = "ok  " if match else "MISMATCH"
        line = (
            f"  [{mark}] {task_type:<16} reference {ref_time:6.3f}s  "
            f"yours {your_time:6.3f}s  ({speedup:.1f}x)"
        )
        print(line)
        lines.append(line)

    return lines


def append_log(blocks: list[list[str]]) -> None:
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
    parts = [f"=== {stamp} ==="]
    for block in blocks:
        parts.extend(block)
        parts.append("")
    BENCHMARK_DIR.mkdir(parents=True, exist_ok=True)
    with BENCHMARK_LOG.open("a", encoding="utf-8") as fh:
        fh.write("\n".join(parts) + "\n")
    print(f"\nAppended timings to {BENCHMARK_LOG.relative_to(ROOT)}")


def main() -> int:
    print("Calibrating machine rates (contractnet.benchmark)…")
    rates = calibrate(verbose=True)
    score = overall_score(rates)
    calib_lines = [
        "calibration (units/s):",
        *[f"  {task:<16} {rate:,.0f}" for task, rate in rates.items()],
        f"  overall_score     {score:,.0f}",
    ]
    print(f"  overall_score     {score:,.0f}")

    print()
    ref_lines = time_reference()
    custom_lines = time_custom_execute()
    append_log([calib_lines, ref_lines, custom_lines])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
