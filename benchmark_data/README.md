# Benchmark results

Run `python verify.py` from the repository root to check the reference answers
and append timings to `verify_benchmarks.log` in this directory.

Each successful run records its UTC timestamp, reference timings, and any
custom executor comparison. Failed checks do not append a result.

The existing benchmark log is kept here with its history intact. New timings
are appended without overwriting earlier results.
