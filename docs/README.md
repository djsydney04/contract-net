# Implementation and change guide

This guide explains the implementation, design decisions, and validation behind
the Rust migration and bidder improvements. It connects the runtime behavior to
the relevant code, tests, and measurements, covering the work merged through
[PR #7](https://github.com/djsydney04/contract-net/pull/7) on September 9, 2026.

The project now runs as a Rust application. It can complete all five task types,
learn from completed work, and adjust its bids using public auction information.
Tests check the answers and connection behavior. Saved measurements and graphs
show execution speed and simulated bidding results.

## Read the guide

| Page | What it explains |
|---|---|
| [Rust migration and project structure](changes/01-rust-application.md) | Native execution, reference compatibility, task algorithms, build commands, and calibration |
| [Function optimization history](changes/06-function-optimizations.md) | Each task from the original reference through optimized Python and Rust, with algorithm pseudocode and source revisions |
| [Protocol decoding and connection recovery](changes/02-connection-fixes.md) | The decimal decoding fix, worker scheduling, reconnects, and persistence boundaries |
| [Bidder learning and pricing](changes/03-bidding.md) | Cost estimation, learned runtime reserves, competition-aware prices, and proposal revisions |
| [Bidding agent pseudocode](changes/07-bidding-pseudocode.md) | The current event flow, forecast calculations, auction checks, price selection, repricing, and settlement updates |
| [Validation, benchmarks, and backtests](changes/04-tests-and-results.md) | Test coverage, percentile measurements, baseline selection, graph interpretation, and evaluation limits |
| [Proposed performance improvements](changes/05-next-improvements.md) | Further implementation and evaluation work; these features are not implemented yet |

## The changes at a glance

| Earlier behavior | Current behavior |
|---|---|
| Python ran the contractor and its tasks. | Rust runs the client, tasks, verifier, benchmarks, and graph tools. |
| Test scripts and generated files cluttered the project. | Application code, tests, documentation, runtime data, and graphs have named folders. |
| Some valid manager messages containing decimals failed to decode. | Message decoding preserves decimal fields and arbitrary-precision integers. |
| Bids mostly covered time spent calculating the answer. | Bids account for estimated delivery and waiting time as well as computation. |
| Completed contracts did not update a saved timing model. | The client keeps up to 512 valid timing and settlement observations. |
| Pricing used a fixed markup on local computation. | Pricing also considers the best visible competing bid and a minimum acceptable margin. |
| A few recorded timings gave a limited view of speed. | Repeated measurements, percentiles, individual-run graphs, and raw samples are saved. |
| The old bidder comparison used an abbreviated pricing formula. | The replay now compiles the actual archived pre-optimization Rust bidder. |

## Where the work appears in history

These are the original implementation commits. The descriptions summarize their
purpose; the topic pages explain the resulting behavior.

| Commit | Change |
|---|---|
| `7607ef1` | Replaced the Python application with Rust, removed the tracked Python environment, and organized tests and documentation. |
| `4cf32c1` | Added repeated Rust benchmarks, saved samples, percentile reports, and graph generation. |
| `031ac8e` | Fixed manager-message decoding for decimal fields while preserving large integers. |
| `71ed371` | Added individual-run, distribution, and rolling-percentile line graphs. This is also the frozen version before bidder optimization. |
| `f3223b0` | Added saved timing learning, public competing-bid observations, revised pricing, market capture, and bidder backtests. |
| `e35f352` | Added varying-delivery backtests and graphs comparing actual archived timing residuals with a fixed assumption. |
| `79fddc2` | Replaced the simplified baseline with the archived Rust policy and added clearer before/after price and cost graphs. |

The native application first reached `main` through
[PR #4](https://github.com/djsydney04/contract-net/pull/4). The remaining benchmark,
graph, and bidder work was brought together in PR #7. Earlier executor shortcuts
and benchmark-log organization already existed before the Rust rewrite; the
rewrite preserved those useful ideas.

## More detailed references

The [main README](../README.md) contains commands and a code map. The
[manager protocol](PROTOCOL.md) describes messages exchanged with the server.
The [task latency report](../graph/README.md) and
[bidder report](../graph/bidder/README.md) contain the saved measurements and
reproduction commands. This guide explains those records; it does not represent
a new benchmark run or new live tournament results.
