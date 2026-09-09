# contractor-net

Rust client for the CPSC 370 Contract Net tournament. The client, bidding
strategy, calibration, five compute tasks, and verification are all native Rust.

## Build and verify

Requires Rust 1.94 or newer.

```bash
cargo build --locked --release
cargo run --locked --release --bin verify
cargo test --locked --all-targets
```

Verification checks the five original golden answers and appends successful
timings to `tests/benchmarks/verify_benchmarks.log`. Check all 218 reference cases
without writing a log:

```bash
cargo run --locked --release --bin verify -- --all --no-log
```

## Run

Use the practice URL and token from Canvas:

```bash
cargo run --locked --release -- \
  --name YourTeamName \
  --url 'wss://PRACTICE_HOST/agent?room=PRACTICE_ROOM' \
  --token CLASS_TOKEN
```

Or run `target/release/contractor-net` with the same arguments. `--machine`
sets the leaderboard label; `--quiet` suppresses routine logs. Ctrl-C stops
the client. Use release builds for calibration and tournament runs.

Keep the same team name for practice, the tournament, and submission. Names
must have 2–24 ASCII letters, digits, underscores, or hyphens and start with a
letter or digit. Open the manager's `/dev` dashboard to inspect your messages.

## Code

| File | Responsibility |
|---|---|
| `src/main.rs` | CLI and shutdown |
| `src/strategy.rs` | Bidding decisions and optional hooks |
| `src/client.rs` | Connection, reconnects, serial worker, queue, settlements |
| `src/protocol.rs` | Wire messages, tasks, bids, rules, and settlements |
| `src/tasks.rs` | Exact task executors |
| `src/random.rs` | Reference-compatible seeded random generator |
| `src/benchmark.rs` | Calibration and work estimates |
| `tests/integration/` | CLI, mock-manager, executor, and strategy tests |
| `tests/support/` | Shared reference-fixture loader |
| `tests/fixtures/` | Pinned answers and their provenance |
| `tests/verify/` | Verification command and benchmark logging |
| `tests/benchmarks/` | Preserved benchmark history |
| `docs/PROTOCOL.md` | Manager protocol |
| `docs/THIRD_PARTY_NOTICES.md` | Third-party attribution |

See [tests/README.md](tests/README.md) for test commands and suite details.

## Performance

[Graphs and latency report](graph/README.md) show measured Rust p50, p95, and
p99 for each task, plus a separately labeled comparison with existing
historical timings. All new benchmarking and graph generation run in Rust.
Raw samples and tooling live under `tests/benchmarks/`.
The report also plots latency by run, cumulative latency distributions, and
rolling p50/p95/p99 across 100-run windows for all five tasks.

```bash
cargo run --locked --release --features benchmark-tools --bin benchmark -- \
  --samples 1000 --warmup 25
```

The optional `benchmark-tools` feature enables Rust graph generation. The
contractor's default build does not include plotting dependencies.

## Runtime behavior

Edit `MyContractor` in `src/strategy.rs` to change bidding. Its `BidContext`
exposes live rules, rates, queue time, settlement history, and profit. The
default strategy preserves the original 60% cost markup and budget/deadline
checks. `Strategy` also provides `execute`, `on_registered`, `on_reject`,
`on_settled`, and `on_bid_invalid` hooks. If you change the executor's work
model, update calibration to match.

The client executes awards serially off the network event loop. It registers
on every connection, applies live rules, handles open auctions, and sends
application keepalives. Unsent results survive reconnects in memory; stale
bids are discarded. Duplicate-name eviction stops the process, and invalid
credentials return an error. There is no recovery after the process exits.

The executors preserve the answer key's seeded random sequence and exact
integer results, including large seeds and matrix moduli. Matrix checksums
use O(n²) work and O(n) storage; prime counting uses a segmented sieve.
Calibration measures these native implementations. Hash estimates remain
expected times, and large-modulus estimates are approximate. Prime bounds
must fit u64; task dimensions must fit available address space and memory.

The 218 pinned reference cases include the original golden answers, all
previous executor tests, and additional seed/modulus boundaries. See
`tests/fixtures/README.md` for provenance. Protocol tests use a local mock
manager; connecting to the course room requires its URL and token.

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
```
