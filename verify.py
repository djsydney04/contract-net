#!/usr/bin/env python3
"""
Check that your machine, and your code, agree with the reference.

Run this before you submit, and every time you touch `execute`:

    python verify.py

Two things are checked.

1. **The reference implementations still produce the known-good answers.**
   The manager grades against an answer key built from `contractnet/tasks.py`.
   If that file has been modified, or your Python somehow computes different
   results, every submission you make will be marked wrong. This catches it
   before the tournament rather than during it.

2. **Your `execute` override, if you wrote one, matches the reference.**
   The extra credit is for being *faster*, not different. A single mismatched
   digit is a wrong answer and a fine. This runs both implementations on the
   same inputs and compares, and reports your speedup.

Exit status is 0 if everything matches, 1 otherwise.
"""

from __future__ import annotations

import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Any, Mapping

from contractnet.tasks import run_task

# Appended on every successful verify run so you can compare timings over time.
BENCHMARK_DIR = Path(__file__).resolve().parent / "benchmark_data"
BENCHMARK_LOG = BENCHMARK_DIR / "verify_benchmarks.log"

# Fixed inputs with answers pinned at the time the assignment was written.
# These must never change. If a code change makes one of these fail, the change
# is wrong, not the golden value.
GOLDEN: list[tuple[str, Mapping[str, Any], int]] = [
    ("monte_carlo_pi", {"seed": 12345, "samples": 50000}, 39336),
    ("prime_count", {"lo": 1000000, "hi": 1010000}, 753),
    ("hash_search", {"seed": "cpsc370-golden", "threshold": 65536}, 90984),
    ("sort_checksum", {"seed": 99, "n": 50000}, 1785318802179667385),
    ("matmul_mod", {"seed": 2026, "n": 40, "mod": 1000003}, 629524),
]


def check_reference() -> tuple[bool, list[str]]:
    print("Checking the reference implementations against known answers…")
    ok = True
    lines: list[str] = []
    for task_type, params, expected in GOLDEN:
        started = time.perf_counter()
        actual = run_task(task_type, params)
        elapsed = time.perf_counter() - started
        good = actual == expected
        ok &= good
        mark = "ok  " if good else "FAIL"
        line = f"  [{mark}] {task_type:<16} {elapsed:6.3f}s  {actual}"
        print(line)
        lines.append(line)
        if not good:
            print(f"         expected {expected}")
    return bool(ok), lines


def check_custom_executor() -> tuple[bool, list[str]]:
    """If the student overrode `execute`, make sure it agrees with the reference."""
    try:
        from my_contractor import MyContractor
    except Exception as err:  # noqa: BLE001
        print(f"\nCould not import MyContractor from my_contractor.py: {err!r}")
        return False, []

    from contractnet import Contractor, Task

    if MyContractor.execute is Contractor.execute:
        msg = "No custom execute(). Using the reference implementation."
        print(f"\n{msg}")
        return True, [msg]

    print("\nChecking your custom execute() against the reference…")
    agent = MyContractor.__new__(MyContractor)  # no network, no calibration
    ok = True
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

        try:
            started = time.perf_counter()
            yours = agent.execute(task)
            your_time = time.perf_counter() - started
        except Exception as err:  # noqa: BLE001
            line = f"  [FAIL] {task_type:<16} raised {err!r}"
            print(line)
            lines.append(line)
            ok = False
            continue

        good = str(yours) == str(reference) == str(expected)
        ok &= good
        speedup = ref_time / your_time if your_time > 0 else float("inf")
        mark = "ok  " if good else "FAIL"
        line = (
            f"  [{mark}] {task_type:<16} reference {ref_time:6.3f}s  "
            f"yours {your_time:6.3f}s  ({speedup:.1f}x)"
        )
        print(line)
        lines.append(line)
        if not good:
            print(f"         yours    {yours}")
            print(f"         expected {expected}")

    return bool(ok), lines


def append_benchmark_log(ref_lines: list[str], custom_lines: list[str]) -> None:
    stamp = datetime.now(timezone.utc).strftime("%Y-%m-%d %H:%M:%S UTC")
    block = [
        f"=== {stamp} ===",
        "reference:",
        *ref_lines,
        *custom_lines,
        "",
    ]
    BENCHMARK_DIR.mkdir(parents=True, exist_ok=True)
    with BENCHMARK_LOG.open("a", encoding="utf-8") as fh:
        fh.write("\n".join(block) + "\n")
    print(f"\nAppended timings to {BENCHMARK_LOG.relative_to(Path(__file__).resolve().parent)}")


def main() -> int:
    reference_ok, ref_lines = check_reference()
    if not reference_ok:
        print(
            "\nThe reference implementations do NOT match the known answers.\n"
            "Restore contractnet/tasks.py from the course repository. Your\n"
            "submissions would all be graded wrong."
        )
        return 1

    custom_ok, custom_lines = check_custom_executor()
    if not custom_ok:
        print(
            "\nYour execute() does not agree with the reference. In the\n"
            "tournament this is scored as a wrong answer and fined."
        )
        return 1

    append_benchmark_log(ref_lines, custom_lines)
    print("\nAll checks passed.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
