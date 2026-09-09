use clap::Parser;
use std::path::PathBuf;

#[path = "../support/fixtures.rs"]
mod fixtures;
mod runner;

#[derive(Parser)]
#[command(about = "Check Rust task results against pinned Python reference answers")]
struct Args {
    /// Also run randomized and edge-case fixtures.
    #[arg(long)]
    all: bool,
    /// Verify without appending a benchmark entry.
    #[arg(long, conflicts_with = "log")]
    no_log: bool,
    /// Destination for append-only timing history.
    #[arg(long)]
    log: Option<PathBuf>,
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let log = args
        .log
        .unwrap_or_else(|| PathBuf::from("tests/benchmarks/verify_benchmarks.log"));
    runner::verify((!args.no_log).then_some(log.as_path()), args.all)?;
    Ok(())
}
