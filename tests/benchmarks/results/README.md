# Rust latency samples

`rust.json` contains 1,000 individual Rust executions per task, recorded in
nanoseconds and in measurement order. It includes source, runtime, machine,
timing scope, warmup count, and measurement timestamps.

The native Rust benchmark constructs the five golden tasks once, performs
25 warmup rounds, then executes each task once in every measured round. It
rotates task order each round and runs serially. Each `execute(task)` call
is timed with `std::time::Instant`; answer validation happens after the clock
stops. No samples are discarded. Rust uses the release profile with thin LTO
and one codegen unit.

The p50, p95, and p99 use empirical nearest rank: `ceil(p * n) - 1` in the
sorted observations. The fixed hash-search seed repeats the same search
length, so these measurements do not represent varying workload difficulty.

## Historical context

The improvement graph reads the existing `verify_benchmarks.log`, specifically
the last complete `custom execute() vs reference` block. Those old values are
single observations rounded to 1 ms. They are displayed separately from the
new Rust percentile measurements. No old executor is launched, and no old
p50/p95/p99 values are invented. The recorded prime timing is zero after
rounding, so a historical improvement ratio cannot be computed for that task.

## Run

```bash
cargo run --locked --release --features benchmark-tools --bin benchmark -- \
  --samples 1000 --warmup 25
```

Add `--render-only` to regenerate graphs from the saved Rust samples without
new measurements. All benchmark and plotting code is Rust.
