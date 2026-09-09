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

## Bidder backtest and decision latency

The [bidder report](../../graph/bidder/README.md) compares the previous and
adaptive policies on earlier public history and a newer 25-task capture. It
uses measured native compute times and explicit 30/50/100 ms delivery-cost
scenarios, holds competitors fixed, and learns only after simulated wins.
The report also measures p50/p95/p99 decision latency with 512 observations.

Three additional figures inspect the delivery assumption: `delivery-overhead`
shows each archived timing residual and the bidder's allowance before that
auction; `delivery-distribution` shows individual task groups and the empirical
distribution; `variable-delivery-profit` compares fixed 50 ms overhead with the
actual archived residual sequence applied equally to both policies. These
residuals come from another contractor and include unseparated queue/processing
and rounded manager timings; they are a scenario proxy, not our measured RTT.
The varying replay retains chronology and never exposes current/future delays
to the bid decision. CSV, JSON, and analysis source hashes accompany the plots.

```bash
cargo run --locked --release --features benchmark-tools --bin bidder-replay
cargo run --locked --release --features benchmark-tools --bin bidder-replay -- --render-only
```

Capture new public auction data without registering a contractor:

```bash
cargo run --locked --release --features benchmark-tools --bin capture-market -- \
  --url 'wss://PRACTICE_HOST/spectate?room=PRACTICE_ROOM' --seconds 210 \
  --output tests/fixtures/practice-market.json
```

## Verification history

Run `cargo run --locked --release --bin verify` from the repository root to check the reference answers
and append timings to `verify_benchmarks.log` in this directory.

Each successful run records its UTC timestamp, Rust build profile, and timings
against the pinned reference answers. Failed checks do not append a result.

Use `-- --all` for all 218 reference fixtures, `-- --no-log` to verify without
writing, or `-- --log PATH` to choose another destination.

The existing benchmark log is kept here with its history intact. New timings
are appended without overwriting earlier results.
