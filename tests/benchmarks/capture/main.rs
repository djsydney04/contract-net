use anyhow::{Result, ensure};
use clap::Parser;
use contractnet::{
    bidder::task_key,
    market::{self, Snapshot},
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, path::PathBuf, time::Duration};

#[derive(Parser)]
#[command(about = "Capture public auction bids without registering a contractor")]
struct Args {
    #[arg(long)]
    url: String,
    #[arg(long, default_value_t = 200)]
    seconds: u64,
    #[arg(long, default_value = "tests/fixtures/practice-market.json")]
    output: PathBuf,
}

#[derive(Deserialize, Serialize)]
struct Capture {
    source: String,
    captured_at: String,
    description: String,
    auctions: Vec<Snapshot>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    ensure!(args.seconds > 0, "capture duration must be positive");
    let mut receiver = market::subscribe(args.url.clone());
    let stop = tokio::time::sleep(Duration::from_secs(args.seconds));
    tokio::pin!(stop);
    let mut auctions = BTreeMap::new();
    loop {
        tokio::select! {
            _ = &mut stop => break,
            result = receiver.changed() => {
                result?;
                let Some(snapshot) = receiver.borrow_and_update().clone() else { continue; };
                for auction in &snapshot.active {
                    if auction.state == "bidding" && !snapshot.live_bids.is_empty()
                        && auction.bids_close_at.is_some_and(|close| close > snapshot.now.saturating_add(200)) {
                        let key = task_key(&auction.task);
                        if !auctions.contains_key(&key) { println!("Captured task {} ({})", auction.task.task_id, auction.task.task_type); }
                        auctions.insert(key, snapshot.clone());
                    }
                }
            }
        }
    }
    ensure!(!auctions.is_empty(), "no public auctions received");
    if let Some(parent) = args.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let capture = Capture { source: args.url, captured_at: chrono::Utc::now().to_rfc3339(),
        description: "Read-only public practice snapshots, taken during bidding; one latest observed snapshot per exact task input. No contractor registered and no bids submitted.".into(),
        auctions: auctions.into_values().collect() };
    fs::write(&args.output, serde_json::to_string_pretty(&capture)? + "\n")?;
    println!(
        "Saved {} auctions to {}",
        capture.auctions.len(),
        args.output.display()
    );
    Ok(())
}
