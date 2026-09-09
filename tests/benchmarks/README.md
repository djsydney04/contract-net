# Benchmark results

Run `cargo run --locked --release --bin verify` from the repository root to check the reference answers
and append timings to `verify_benchmarks.log` in this directory.

Each successful run records its UTC timestamp, Rust build profile, and timings
against the pinned reference answers. Failed checks do not append a result.

Use `-- --all` for all 218 reference fixtures, `-- --no-log` to verify without
writing, or `-- --log PATH` to choose another destination.

The existing benchmark log is kept here with its history intact. New timings
are appended without overwriting earlier results.
