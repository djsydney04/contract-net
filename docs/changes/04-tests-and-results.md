# Validation, benchmarks, and backtests

[Back to the guide](../README.md)

## Reference checks and integration coverage

The Rust tasks are checked against 218 saved answers from the original
reference implementation. These include the five original example tasks,
earlier executor tests, and extra cases involving large or negative seeds and
large matrix values. The expected answers were not rewritten to match Rust.

Integration tests use local mock managers to check registration, decimal message
fields, large integers, bidding, execution, reconnects, repeated awards, and
saved learning. One test runs both a mock manager and a mock spectator so it can
check that changing public bids leads to a revised proposal without duplicate
messages. Command-line tests launch the compiled Rust program.

The implementation, including the overlapping-auction safeguards, passed 29
integration tests, the benchmark and
replay percentile tests, the old-source integrity test, and the paired-delay
test described below. Release verification also passed all 218 answer checks.
These are records of the implementation validation, not additional test runs
performed while writing this guide.

GitHub's automated checks run formatting, Rust code checks, all-feature tests,
and release answer verification on Linux. Tests use local mock connections;
passing them does not demonstrate a live tournament win.

Run the checks from the project root:

```bash
cargo fmt --all -- --check
cargo clippy --locked --all-features --all-targets -- -D warnings
cargo test --locked --all-features --all-targets
cargo run --locked --release --bin verify -- --all --no-log
```

## Measurement boundaries and percentiles

**Task execution time** is time spent calculating an answer inside the Rust
process. **Bid decision time** is time spent choosing a proposal. **Manager
elapsed time** includes the period from award to result arrival and is what
drives the bill. They are different measurements and should not be added or
compared as though they describe the same activity.

P50 is the sample median. P95 is a time at or above 95% of the recorded
attempts, and p99 is at or above 99%. The slower end of a sample matters because
an occasional slow task can miss a deadline even when the usual task is fast.
A percentile from a saved sample does not guarantee the behavior of future runs.

The tables label their units: milliseconds (ms) are thousandths of a second;
microseconds (µs) are millionths of a second.

## Task execution measurements

The saved Rust benchmark used 25 warmups followed by 1,000 timed executions of
each of five fixed workloads. Every answer was checked. It retained slow
observations rather than removing them. This run used an Apple M3 Pro and a
release build with Rust 1.94.0.

| Task | p50, milliseconds | p95, milliseconds | p99, milliseconds |
|---|---:|---:|---:|
| Monte Carlo | 0.4903 | 0.7549 | 1.0290 |
| Prime counting | 0.0204 | 0.0255 | 0.0585 |
| Hash search | 17.9824 | 20.3596 | 34.0547 |
| Sorting checksum | 1.6935 | 2.0116 | 2.9580 |
| Matrix checksum | 0.0439 | 0.0488 | 0.0821 |

These values exclude networking, queue time, startup, task construction,
serialization, and answer checking. The hash workload uses the same seed on
every attempt, so its timing spread does not measure difficulty across new
hash inputs.

The historical executor comparison uses older log entries containing single
times rounded to milliseconds. Those are not old p50, p95, or p99 measurements.
We did not run a fresh Python benchmark or invent its missing percentiles.
The old prime-count time rounded to zero, so a meaningful speedup ratio cannot
be calculated for that entry.

See [the execution report](../../graph/README.md) for exact inputs, dates,
environment details, raw samples, and the approximate old-time comparison.

## Bid decision measurements

The decision benchmark called the real optimized bidding function with a full
512-observation history and saved market information. It measured 1,000
decisions for each of 25 captured tasks after warmup: 25,000 decisions total,
combined below into 5,000 observations per task type.

| Task | p50, microseconds | p95, microseconds | p99, microseconds |
|---|---:|---:|---:|
| Hash search | 4.167 | 4.291 | 5.125 |
| Matrix checksum | 4.167 | 4.250 | 4.375 |
| Monte Carlo | 3.917 | 4.000 | 4.125 |
| Prime counting | 4.000 | 4.125 | 4.250 |
| Sorting checksum | 3.875 | 4.000 | 4.125 |

This measures choosing a bid. It excludes task execution, file writes, network
delivery, and queue handling. It is not a before/after measurement of the whole
client's speed. The [bidder report](../../graph/bidder/README.md) links to every
saved timing and the source information used to identify that measured code.

## What the backtest does

A backtest asks how different bidding policies would behave on saved auctions.
Here, it uses measured local calculation times, recorded competing bids, and
an explicit assumption about additional delivery time to calculate hypothetical
wins, costs, and profit. These are modeled results, not money earned by running
both clients live.

The recent dataset contains all 25 practice tasks, captured by watching the
public spectator feed for 210 seconds. The collector did not register a team
or submit bids. Each task was calculated once for warmup and five times for
measurement. The replay uses the middle local time for its cost calculation.

The older dataset contains 11 complete auctions transcribed from an earlier
public API trace. The full temporary snapshot was no longer available, and the
trace did not include full task inputs. We reused inputs from the later frozen
task pool only when task number, type, budget, deadline, and the computed answer
all matched the older record. All 11 included records matched. They cover four
task types, with no historical hash-search cases.

This reconstruction is a documented assumption. It is not a complete archive
of the old tournament. The [fixture notes](../../tests/fixtures/README.md) and
[historical data](../../tests/fixtures/practice-history.json) preserve its origin.

Each older auction runs once in its original order. Learning starts empty and
is updated only after the optimized policy's own simulated wins. The current
or future delay is not supplied to the bid decision. A test changes the delay
for one auction and checks that both bidders' costs change equally while their
already chosen bids stay unchanged.

## Which old model is used

The “Before: 71ed371” line executes the actual Rust strategy immediately before
the bidder optimizations. Its source is stored unchanged in
[the archived bidder file](../../tests/fixtures/bidder-before-optimization.rs).
The replay compiles that file and checks its SHA-256 hash against the pinned
value. This verifies that the archived source has not been modified.

“After: optimized” runs the current strategy. Both policies receive the same
saved calibration, task measurements, competitor information, and realized
delivery assumptions. The old policy simply does not use the added learning
and market information. This comparison isolates the bidding rules, rather
than comparing two entire historical installations or Python against Rust.

## Results on the 11 older auctions

The fixed scenarios apply the named delay to every auction. The varying
scenario uses each older record's manager time minus its reported calculation
time. For any particular auction, both policies face the same realized delay.

| Additional-time scenario | Before modeled profit | After modeled profit | Before / after wins | Before / after losing contracts |
|---|---:|---:|---:|---:|
| Fixed 30 ms | $0.1878 | $37.0502 | 11 / 11 | 5 / 0 |
| Fixed 50 ms | -$0.0322 | $36.6702 | 11 / 11 | 6 / 0 |
| Fixed 100 ms | -$0.5822 | $35.6432 | 11 / 10 | 10 / 0 |
| Varying archived timing | -$0.1025 | $36.5346 | 11 / 11 | 7 / 0 |

The varying amounts range from 42.4 to 68.9 ms, with a middle value of 55.3 ms.
They belong to another contractor. They may include waiting and processing,
and the manager times were rounded to 10 ms. They are not measurements of our
own network. With only 11 observations, p95 and p99 both pick the maximum;
that is not enough evidence to establish dependable tail latency or differences
between task types.

The newer 25-task dataset was also replayed for three cycles, making 75 auctions.
At 50 ms assumed extra time, modeled profit was $5.1088 before and $341.9408
after, with 75 wins each and 42 versus 0 losing contracts. The newer competing
bids were more expensive, so this result is reported separately from the older
history. Repeating a dataset does not create 75 independent historical auctions.

Both evaluations hold competing bids fixed. They do not model competitors
reacting to our prices, every possible delay spike, or overlapping live work.
Late results use the configured reduced payment and penalty rules. No result
here guarantees future wins, zero losses, or the same profit in a live room.

## What each graph shows

The images are saved as SVG and PNG. SVG scales cleanly; PNG is convenient for
sharing. The links below open the PNG versions.

| Graph | How to read it |
|---|---|
| [Task percentiles](../../graph/latency-percentiles.png) | Compares p50, p95, and p99 execution times for each fixed workload. |
| [Every execution](../../graph/latency-runs.png) | Shows all measured attempts in order, including unusually slow ones. |
| [Execution distribution](../../graph/latency-distribution.png) | Shows what fraction of attempts finished within a given time. |
| [Rolling percentiles](../../graph/latency-rolling-percentiles.png) | Recalculates percentiles over the last 100 attempts to show variation within the run. |
| [Historical execution comparison](../../graph/historical-comparison.png) | Compares old rounded single timings with the Rust middle time; it is approximate. |
| [Older auction profit](../../graph/bidder/historical-profit.png) | Compares before/after cumulative modeled profit at 30, 50, and 100 ms extra time. |
| [Recent auction profit](../../graph/bidder/cumulative-profit.png) | Shows the same comparison on the newer 25-task capture repeated three times. |
| [Delivery allowance](../../graph/bidder/delivery-overhead.png) | Compares archived timing variation, the old zero allowance, the fixed assumption, and the learned allowance available before each decision. |
| [Delivery distribution](../../graph/bidder/delivery-distribution.png) | Shows the individual observations by task type and their overall distribution. |
| [Fixed versus varying delivery](../../graph/bidder/variable-delivery-profit.png) | Compares profit under a constant delay and the archived sequence of varying delays. |
| [Bid prices and costs](../../graph/bidder/bid-price-comparison.png) | Shows both policies' prices, then a separate detail scale showing where old bids fell below modeled costs. |

Rolling lines show variation within a saved run, not proof of improvement from
one code revision to another. A rising profit line is cumulative modeled profit,
not an improving execution speed. The reports label these distinctions.

## Reusing and refreshing the evidence

Raw timings, source hashes, environment details, individual replay
outcomes, and CSV summaries are saved alongside the reports. The graph follow-ups
reused existing timing samples; adding a new view did not silently create a new
speed measurement.

These commands redraw reports from the saved samples:

```bash
cargo run --locked --release --features benchmark-tools --bin benchmark -- --render-only
cargo run --locked --release --features benchmark-tools --bin bidder-replay -- --render-only
```

They write regenerated graph and report files. The bidder command runs the
comparison using current analysis code and the frozen input data. For fresh
measurements or a new public capture, follow
[the benchmark instructions](../../tests/benchmarks/README.md). Use separate
output paths when keeping results from different runs for comparison.
