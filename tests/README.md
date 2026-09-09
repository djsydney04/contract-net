# Tests and verification

All test code, reference data, verification tooling, and benchmark history live
in this folder. The application library in `src/` contains only runtime code.

| Folder | Contents |
|---|---|
| `integration/` | Cargo test harness and suites for the CLI, WebSocket client, executors, and bidding strategy |
| `support/` | Fixture loader shared by the test harness and verifier |
| `fixtures/` | 218 pinned reference answers and provenance |
| `verify/` | Standalone `verify` binary and benchmark logger |
| `benchmarks/` | Verification history, repeated-latency runner, and raw benchmark samples |

Run commands from the repository root:

```bash
# All tests, including local WebSocket and CLI exchanges
cargo test --locked --all-targets

# One suite
cargo test --locked --test integration client::
cargo test --locked --test integration executor::

# Five golden answers, appending release timings to tests/benchmarks/
cargo run --locked --release --bin verify

# All 218 answers without writing a log
cargo run --locked --release --bin verify -- --all --no-log
```

The `integration` target is registered in `Cargo.toml`; its `main.rs` loads the
four suites. Tests bind temporary loopback ports and do not contact the course
manager. The `verify` binary shares fixture data with the tests and runs the
same `MyContractor` executor as the application.

Repeated performance measurements and graph generation are documented in
[benchmarks/README.md](benchmarks/README.md). Run tests for the percentile
calculations with `cargo test --locked --all-features --all-targets`.
