# The Rust application and folder cleanup

[Back to the guide](../README.md)

## The application now runs in Rust

The original application used Python to connect to the manager, bid on work,
calculate answers, and check results. Those responsibilities now run in Rust.
Python is not needed to build or run the contractor, tests, benchmarks, or graph
generator.

The tracked Python source files, dependency file, virtual environment, and
compiled Python cache files were removed during the migration. Ignore rules
keep local environments, build output, and caches from being added again.
The saved reference answers and old timing logs remain because they are useful
evidence about correctness and history.

Some names and notices still mention Python. In particular, the Rust random
number generator must reproduce the original Python sequence so the manager
accepts our answers. That compatibility code is written in Rust. Its attribution
is kept in [the third-party notices](../THIRD_PARTY_NOTICES.md).

## The five tasks still return the expected answers

Moving to another language must not change an answer. Given the same task
inputs, the program must draw the same random numbers and keep whole-number
calculations exact, even when values are very large.

| Task | What it does and how the Rust version handles it |
|---|---|
| `monte_carlo_pi` | Draws repeatable random points and counts how many fall inside a circle. It preserves the reference's draw order. |
| `prime_count` | Counts primes by crossing out multiples in manageable sections of the requested range. This avoids testing every number independently. |
| `hash_search` | Tries candidate numbers in order until a hash meets the target. It reuses the fixed beginning of the hash and avoids creating a new text allocation for every attempt. |
| `sort_checksum` | Generates the same numbers as the reference, sorts them, and calculates the required whole-number checksum. |
| `matmul_mod` | Calculates the required matrix checksum using column and row totals, avoiding the work of constructing a full matrix product. |

Several of these shortcuts were already present in the optimized Python
executor. We carried them into Rust. For the matrix task, doubling the matrix
dimension means roughly four times as much core work rather than the eightfold
growth of ordinary full matrix multiplication. Only a row-sized collection of
totals needs to be kept.

Large seeds and matrix values remain exact. Inputs that exceed supported bounds
or available memory can still fail; Rust does not make resources unlimited.
The [reference fixtures](../../tests/fixtures/README.md) explain the 218 saved
answers used to check compatibility.

## Files have clear homes

| Folder or file | Purpose |
|---|---|
| `src/` | The application: commands, connection handling, bidding, learning, and task calculations |
| `tests/integration/` | Tests that exercise the application and its parts together |
| `tests/fixtures/` | Saved answer keys, public auction records, and the frozen old bidder |
| `tests/support/` | Helpers shared by tests and verification |
| `tests/verify/` | The command that checks task answers |
| `tests/benchmarks/` | Measurement, auction capture, replay, and graph-generation tools, plus saved measurements |
| `docs/` | This guide, message documentation, and attribution |
| `data/runtime/` | Local learned observations; these files are ignored by Git |
| `graph/` | Saved charts, reports, and table exports |
| `Cargo.toml` and `Cargo.lock` | Rust build settings and the selected dependency versions |

The private practice launcher used locally is not part of the shared changes.
The documented commands use placeholders for the room and token.

## Building and running it

Use Rust 1.94 or newer. Run these commands from the project root:

```bash
cargo build --locked --release --bin contractor-net
target/release/contractor-net \
  --name YourTeamName \
  --url 'wss://PRACTICE_HOST/agent?room=PRACTICE_ROOM' \
  --token CLASS_TOKEN
```

Replace the placeholders with the class-provided values. `--release` builds the
optimized application used for the performance measurements. After changing
application code, rebuild and restart the process to use the new binary. A
process that is already running keeps using its old code.

Graph libraries are optional. They are included when a benchmark command enables
`--features benchmark-tools`; the ordinary contractor build does not need them.

## Startup measurements are steadier

At startup, the contractor measures the computer's speed for each task type.
This gives it a starting estimate of how much work it can finish in a second.
Instead of trusting one timed attempt, calibration now performs two warmups
followed by five timed attempts and uses the middle result. This reduces the
influence of one unusually slow or fast attempt.

Calibration still varies with the machine and its load. Deterministic answers
do not imply identical wall-clock timing on every run.
