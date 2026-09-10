# contractor-net

Rust client for the CPSC 370 Contract Net tournament. The client, bidding
strategy, calibration, five compute tasks, and verification are all native Rust.

For the implementation details and design decisions, start with the
[change guide](docs/README.md). It covers the Rust migration, connection handling,
bidder logic, validation, performance results, and proposed improvements.

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
| `src/bidder.rs` | Learned timing estimates and saved observations |
| `src/market.rs` | Read-only public auction observations |
| `src/client.rs` | Connection, reconnects, serial worker, queue, settlements |
| `src/protocol.rs` | Wire messages, tasks, bids, rules, and settlements |
| `src/tasks.rs` | Exact task executors |
| `src/random.rs` | Reference-compatible seeded random generator |
| `src/benchmark.rs` | Calibration and work estimates |
| `tests/integration/` | CLI, mock-manager, executor, and strategy tests |
| `tests/support/` | Shared reference-fixture loader |
| `tests/fixtures/` | Pinned answers and their provenance |
| `tests/verify/` | Verification command and benchmark logging |
| `tests/benchmarks/` | Measurement, market capture, backtests, graph tools, and saved results |
| `docs/README.md` | Implementation guide and change history |
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
exposes live rules, rates, queue time, learned runtimes, public competing bids,
settlement history, and profit. The default strategy prices manager-billed
delivery time, including network and queue overhead. It learns from local
execution times and manager settlements, uses a conservative completion
forecast to admit work, and keeps a profit margin above that cost forecast.
For `best_value`, it prices just below the best visible competing score while
preserving that margin. Decisions are deterministic for identical inputs,
calibration, learned observations, and market snapshots.

The read-only spectator connection updates competing bids during the auction;
only changed proposals are sent. Stale, invalid, own, and mismatched-auction
bids are excluded. If the feed is unavailable, the bidder falls back to
cost-based pricing with an 8% budget floor. Use `--no-market-feed` to disable
the spectator connection. No winning or profit guarantee is implied by the
forecast; runtime and competitors can change after a proposal.

Learning is saved under `data/runtime/` by manager URL, room, and team name.
Only the latest 512 valid execution/settlement observations are retained.
Use `--state PATH` to choose the file, or `--no-state` to keep learning in memory.
The initial delivery-overhead assumption is 50 ms plus 5 ms margin, replaced
by the observed p95 once settlements arrive. Calibration uses two warmups and
the median of five timed runs. Unknown hash-search inputs reserve approximately
the 95th-percentile search time for deadline admission; repeated exact inputs
learn their observed difficulty. Quoted completion uses an expected/median
compute estimate plus the delivery allowance, while pricing and queue capacity
use the more conservative reserve.

`Strategy` also provides `compute_reserve`, `execute`, `on_registered`, `on_reject`,
`on_settled`, and `on_bid_invalid` hooks. If you change the executor's work
model, update calibration to match.

The client executes awards serially off the network event loop. It registers
on every connection, applies live rules, handles open auctions, and sends
application keepalives. Unsent results survive reconnects in memory; stale
bids are discarded. Duplicate-name eviction stops the process, and invalid
credentials return an error. There is no recovery after the process exits.

For simultaneous auctions, bids reserve waiting time using the manager's
`concurrency` rule and the remaining committed work. Admission protects earlier
quotes even when awards arrive out of order. Jobs that exceed their compute
reserve stop new bidding until the worker finishes, including after a manager
timeout. See [queue accounting](docs/changes/03-bidding.md#overlapping-auctions-and-the-execution-queue)
for the formula and tradeoffs.

Learned runtime observations survive restarts; outstanding contracts do not.
See the [bidder backtest and decision-latency report](graph/bidder/README.md)
for archived practice data, delivery-cost assumptions, and replay results.

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
