# Benchmark results

Run `python verify.py` from the repository root to check the reference answers
and append timings to `verify_benchmarks.log` in this directory.

Each successful run records its UTC timestamp, reference timings, and any
custom executor comparison. Failed checks do not append a result.

Logs are local generated output and are ignored by Git. Existing results are
preserved when new timings are appended.
