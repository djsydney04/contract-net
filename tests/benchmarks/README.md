# Benchmark results

The [latency report](../../graph/README.md) shows measured Rust p50/p95/p99 for
each task and a separately labeled historical comparison. `runner/` contains
the Rust measurement, statistics, and graph code; `results/` stores the raw
nanosecond samples and capture methodology.

The report also includes three line figures, each with a panel per task:

- `latency-runs`: every measurement in order with p50/p95/p99 reference lines.
- `latency-distribution`: empirical cumulative latency curves and percentile markers.
- `latency-rolling-percentiles`: trailing 100-run p50/p95/p99, updated every run.

These views describe variation within the saved run. They use the same raw
samples as the summary; they do not establish improvement across code versions.

```bash
cargo run --locked --release --features benchmark-tools --bin benchmark -- \
  --samples 1000 --warmup 25
```

Add `--render-only` to regenerate `/graph` from existing samples without
running a new benchmark. Both SVG and PNG outputs are generated in Rust.
The historical view reads existing log entries; it does not execute any old
implementation. Use `--data` and `--output` to save a new run separately.

## Verification history

Run `cargo run --locked --release --bin verify` from the repository root to check the reference answers
and append timings to `verify_benchmarks.log` in this directory.

Each successful run records its UTC timestamp, Rust build profile, and timings
against the pinned reference answers. Failed checks do not append a result.

Use `-- --all` for all 218 reference fixtures, `-- --no-log` to verify without
writing, or `-- --log PATH` to choose another destination.

The existing benchmark log is kept here with its history intact. New timings
are appended without overwriting earlier results.
