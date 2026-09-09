use anyhow::Result;
use clap::Parser;
use contractnet::{ClientConfig, Contractor, MyContractor};

#[derive(Parser)]
#[command(version, about = "CPSC 370 Contract Net contractor")]
struct Args {
    /// Team name used for practice, the tournament, and submission.
    #[arg(long)]
    name: String,
    /// Manager WebSocket endpoint (wss://.../agent?room=...).
    #[arg(long)]
    url: String,
    /// Class token, if the room requires one.
    #[arg(long)]
    token: Option<String>,
    /// Machine label shown on the leaderboard.
    #[arg(long)]
    machine: Option<String>,
    /// Suppress calibration and connection logs.
    #[arg(long)]
    quiet: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let mut config = ClientConfig::new(args.name, args.url);
    config.token = args.token;
    config.verbose = !args.quiet;
    if let Some(machine) = args.machine {
        config.machine = machine;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    let result = runtime.block_on(async {
        let mut client = Contractor::new(config, MyContractor);
        tokio::select! {
            result = client.run() => result,
            result = tokio::signal::ctrl_c() => { result?; eprintln!("shutting down"); Ok(()) }
        }
    });
    // A running CPU task cannot be aborted; do not make Ctrl-C wait for it.
    runtime.shutdown_timeout(std::time::Duration::from_millis(100));
    result
}
