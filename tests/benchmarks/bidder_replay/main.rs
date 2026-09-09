use anyhow::{Context, Result, ensure};
use clap::Parser;
use contractnet::{
    Bid, BidContext, MyContractor, Rules, Settlement, Strategy, Task,
    benchmark::{self, Rates, calibrate},
    bidder::{Learning, Observation, task_key},
    market::{Snapshot, score},
    tasks,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap, fmt::Write as _, fs, hint::black_box, path::PathBuf, time::Instant,
};

mod comparison;
mod delivery;
mod graphs;
mod historical;
// Preserve the historical source verbatim, including its crate-relative imports.
#[rustfmt::skip]
#[allow(dead_code)]
#[path = "../../fixtures/bidder-before-optimization.rs"]
mod pre_optimization;
#[path = "../runner/statistics.rs"]
mod statistics;

#[derive(Parser)]
#[command(
    about = "Compare bidder policies against saved public bids, with explicit delivery-cost scenarios"
)]
struct Args {
    #[arg(long, default_value = "tests/fixtures/practice-market.json")]
    capture: PathBuf,
    #[arg(long, default_value = "tests/fixtures/practice-history.json")]
    history: PathBuf,
    #[arg(long, default_value = "tests/benchmarks/results/bidder.json")]
    data: PathBuf,
    #[arg(long, default_value = "graph/bidder")]
    output: PathBuf,
    #[arg(long)]
    render_only: bool,
}

#[derive(Deserialize)]
struct Capture {
    source: String,
    captured_at: String,
    auctions: Vec<Snapshot>,
}

#[derive(Clone, Deserialize, Serialize)]
struct MeasuredTask {
    market: Snapshot,
    task: Task,
    local_samples_ns: Vec<u64>,
    result: String,
    decision_samples_ns: Vec<u64>,
}

#[derive(Deserialize, Serialize)]
struct Measurements {
    version: u32,
    source_commit: String,
    source_files_sha256: BTreeMap<String, String>,
    runtime: String,
    platform: String,
    completed_at: String,
    capture_source: String,
    capture_time: String,
    rates: Rates,
    tasks: Vec<MeasuredTask>,
}

#[derive(Serialize)]
struct Outcome {
    auction: usize,
    task_id: u64,
    task_type: String,
    delivery_ms: f64,
    forecast_delivery_ms: f64,
    cost_if_awarded: f64,
    old_price: Option<f64>,
    new_price: Option<f64>,
    old_profit: f64,
    new_profit: f64,
    old_cumulative: f64,
    new_cumulative: f64,
}

#[derive(Serialize)]
struct Scenario {
    network_ms: Option<f64>,
    old_wins: usize,
    new_wins: usize,
    old_losses: usize,
    new_losses: usize,
    outcomes: Vec<Outcome>,
}

fn context<'a>(task: &'a MeasuredTask, rates: &'a Rates, learning: &'a Learning) -> BidContext<'a> {
    BidContext {
        rules: &task.market.config,
        rates,
        learning,
        market: Some(&task.market),
        history: &[],
        queue_seconds: 0.0,
        name: "TheGoodGuys",
    }
}

fn baseline(task: &Task, context: &BidContext<'_>) -> Option<Bid> {
    let archived_context = pre_optimization::BidContext {
        rules: context.rules,
        rates: context.rates,
        history: context.history,
        queue_seconds: context.queue_seconds,
    };
    pre_optimization::Strategy::on_cfp(&pre_optimization::MyContractor, task, &archived_context)
}

fn measure(args: &Args) -> Result<Measurements> {
    ensure!(!cfg!(debug_assertions), "measurements require --release");
    let capture: Capture = serde_json::from_slice(&fs::read(&args.capture)?)?;
    ensure!(!capture.auctions.is_empty(), "capture has no auctions");
    let rates = calibrate(false)?;
    let mut measured = Vec::new();
    for market in capture.auctions {
        let task = market.active.first().context("missing task")?.task.clone();
        let mut result = None;
        let mut samples = Vec::new();
        for round in 0..6 {
            let start = Instant::now();
            let answer = black_box(MyContractor.execute(black_box(&task)))?;
            let elapsed = u64::try_from(start.elapsed().as_nanos())?;
            let answer = answer.to_string();
            if let Some(expected) = &result {
                ensure!(*expected == answer, "non-deterministic task result");
            }
            result = Some(answer);
            if round > 0 {
                samples.push(elapsed.max(1));
            }
        }
        println!("Measured #{} {}", task.task_id, task.task_type);
        measured.push(MeasuredTask {
            market,
            task,
            local_samples_ns: samples,
            result: result.unwrap(),
            decision_samples_ns: Vec::new(),
        });
    }
    measured.sort_by_key(|m| m.task.task_id);
    // Time the real decision function with a full, bounded learning history.
    // Task execution timings are separate and never part of these samples.
    let mut learning = Learning::default();
    for i in 0..512 {
        let m = &measured[i % measured.len()];
        let local = statistics::summarize(&m.local_samples_ns)?.p50_ms / 1000.0;
        learning.record(Observation {
            task_key: task_key(&m.task),
            task_type: m.task.task_type.clone(),
            baseline_seconds: context(m, &rates, &learning).estimate(&m.task),
            local_seconds: local,
            queue_seconds: 0.0,
            manager_seconds: local + 0.05,
            cost_rate: m.market.config.cost_rate,
            cost: (local + 0.05) * m.market.config.cost_rate,
            profit: 0.1,
        });
    }
    for m in &mut measured {
        let samples = {
            let ctx = context(m, &rates, &learning);
            for _ in 0..25 {
                black_box(MyContractor.on_cfp(black_box(&m.task), black_box(&ctx)));
            }
            (0..1000)
                .map(|_| {
                    let start = Instant::now();
                    black_box(MyContractor.on_cfp(black_box(&m.task), black_box(&ctx)));
                    u64::try_from(start.elapsed().as_nanos())
                        .unwrap_or(u64::MAX)
                        .max(1)
                })
                .collect()
        };
        m.decision_samples_ns = samples;
    }
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()?;
    use sha2::{Digest, Sha256};
    let mut source_files_sha256 = BTreeMap::new();
    for path in [
        "src/bidder.rs",
        "src/strategy.rs",
        "src/benchmark.rs",
        "src/market.rs",
        "tests/benchmarks/bidder_replay/main.rs",
        "tests/benchmarks/bidder_replay/historical.rs",
    ] {
        source_files_sha256.insert(
            path.into(),
            format!("{:x}", Sha256::digest(fs::read(path)?)),
        );
    }
    Ok(Measurements {
        version: 1,
        source_commit: String::from_utf8(commit.stdout)?.trim().into(),
        source_files_sha256,
        runtime: String::from_utf8(
            std::process::Command::new("rustc")
                .arg("--version")
                .output()?
                .stdout,
        )?
        .trim()
        .into(),
        platform: format!(
            "{} {}; release; thin LTO; one codegen unit",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        completed_at: chrono::Utc::now().to_rfc3339(),
        capture_source: capture.source,
        capture_time: capture.captured_at,
        rates,
        tasks: measured,
    })
}

fn settle(bid: Option<Bid>, m: &MeasuredTask, local: f64, network: f64) -> (bool, f64) {
    let Some(bid) = bid else {
        return (false, 0.0);
    };
    let rules = &m.market.config;
    let ours = score(rules, bid.price, bid.est_seconds).unwrap_or(f64::INFINITY);
    let competition = m
        .market
        .competing_score(&m.task, rules, "TheGoodGuys")
        .unwrap_or(f64::INFINITY);
    if ours >= competition {
        return (false, 0.0);
    }
    let cost = (local + network) * rules.cost_rate;
    let profit = if local + network <= m.task.deadline_s {
        bid.price - cost
    } else {
        bid.price * rules.late_credit - cost - rules.penalty_rate * m.task.budget
    };
    (true, profit)
}

fn replay(
    data: &Measurements,
    tasks: &[MeasuredTask],
    network_ms: f64,
    cycles: usize,
) -> Result<Scenario> {
    let mut scenario = replay_delays(data, tasks, &vec![network_ms; tasks.len()], cycles)?;
    scenario.network_ms = Some(network_ms);
    Ok(scenario)
}

fn replay_delays(
    data: &Measurements,
    tasks: &[MeasuredTask],
    delivery_ms: &[f64],
    cycles: usize,
) -> Result<Scenario> {
    ensure!(
        !tasks.is_empty()
            && tasks.len() == delivery_ms.len()
            && cycles > 0
            && delivery_ms.iter().all(|n| n.is_finite() && *n >= 0.0),
        "replay requires one finite nonnegative delay per task and at least one cycle"
    );
    let mut learning = Learning::default();
    let mut scenario = Scenario {
        network_ms: None,
        old_wins: 0,
        new_wins: 0,
        old_losses: 0,
        new_losses: 0,
        outcomes: Vec::new(),
    };
    let (mut old_total, mut new_total) = (0.0, 0.0);
    for _ in 0..cycles {
        for (m, delay) in tasks.iter().zip(delivery_ms) {
            let network = delay / 1000.0;
            let local = statistics::summarize(&m.local_samples_ns)?.p50_ms / 1000.0;
            let ctx = context(m, &data.rates, &learning);
            let forecast_delivery_ms =
                contractnet::bidder::forecast(&m.task, &ctx).delivery_seconds * 1000.0;
            let old = baseline(&m.task, &ctx);
            let new = MyContractor.on_cfp(&m.task, &ctx);
            let (old_won, old_profit) = settle(old, m, local, network);
            let (new_won, new_profit) = settle(new, m, local, network);
            scenario.old_wins += usize::from(old_won);
            scenario.new_wins += usize::from(new_won);
            scenario.old_losses += usize::from(old_profit < 0.0);
            scenario.new_losses += usize::from(new_profit < 0.0);
            old_total += old_profit;
            new_total += new_profit;
            let baseline_seconds = ctx.estimate(&m.task);
            if new_won {
                learning.record(Observation {
                    task_key: task_key(&m.task),
                    task_type: m.task.task_type.clone(),
                    baseline_seconds,
                    local_seconds: local,
                    queue_seconds: 0.0,
                    manager_seconds: local + network,
                    cost_rate: m.market.config.cost_rate,
                    cost: (local + network) * m.market.config.cost_rate,
                    profit: new_profit,
                });
            }
            scenario.outcomes.push(Outcome {
                auction: scenario.outcomes.len() + 1,
                task_id: m.task.task_id,
                task_type: m.task.task_type.clone(),
                delivery_ms: *delay,
                forecast_delivery_ms,
                cost_if_awarded: (local + network) * m.market.config.cost_rate,
                old_price: old.map(|b| b.price),
                new_price: new.map(|b| b.price),
                old_profit,
                new_profit,
                old_cumulative: old_total,
                new_cumulative: new_total,
            });
        }
    }
    Ok(scenario)
}

fn main() -> Result<()> {
    let args = Args::parse();
    comparison::verify_archive()?;
    let data: Measurements = if args.render_only {
        serde_json::from_slice(&fs::read(&args.data)?)?
    } else {
        let data = measure(&args)?;
        if let Some(parent) = args.data.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&args.data, serde_json::to_string_pretty(&data)? + "\n")?;
        data
    };
    ensure!(
        data.version == 1 && !data.tasks.is_empty(),
        "invalid replay dataset"
    );
    let old_cases = historical::cases(&data, &args.history)?;
    let historical_scenarios = [30.0, 50.0, 100.0]
        .into_iter()
        .map(|n| replay(&data, &old_cases.tasks, n, 1))
        .collect::<Result<Vec<_>>>()?;
    let scenarios = [30.0, 50.0, 100.0]
        .into_iter()
        .map(|n| replay(&data, &data.tasks, n, 3))
        .collect::<Result<Vec<_>>>()?;
    fs::create_dir_all(&args.output)?;
    let delivery_doc = delivery::report(&args.output, &data, &old_cases)?;
    fs::write(
        args.output.join("replay.json"),
        serde_json::to_string_pretty(
            &serde_json::json!({"policies":comparison::provenance(),"historical":historical_scenarios,"recent_capture":scenarios}),
        )? + "\n",
    )?;
    graphs::render(
        &args.output,
        &historical_scenarios,
        "historical-profit",
        "Backtest against earlier public bids",
    )?;
    graphs::render(
        &args.output,
        &scenarios,
        "cumulative-profit",
        "Replay against newer public bids",
    )?;
    let mut doc = format!(
        "# Bidder evaluation\n\nThis is a **counterfactual replay**, not live tournament profit. It uses {}\ncaptured public practice auctions repeated for three cycles, with competitor\nbids held fixed. Each workload was executed in release Rust once for warmup\nand five times for measurement; the measured local p50 is used for cost.\nThe adaptive policy starts with no learned observations and learns only after\nits own simulated wins.\n\n![Cumulative profit under explicit delivery-cost scenarios](cumulative-profit.svg)\n\n## Delivery-cost scenarios\n\nThe added 30, 50, and 100 ms are explicit assumptions, not measurements of this\nclient's network. The practice history shows manager runtimes exceeding local\ncompute times. Both policies face the same imposed overhead and actual\ncaptured award configuration. Costs are `(local p50 + overhead) × cost_rate`;\nlate delivery is modeled with `late_credit` revenue and a budget-proportional\n`penalty_rate` fine. Competition does not react to revised bids in this replay.\n\n| Added delivery time | Old profit | New profit | Old/new wins | Old/new losing contracts |\n|---|---:|---:|---:|---:|\n",
        data.tasks.len()
    );
    for s in &scenarios {
        let last = s.outcomes.last().unwrap();
        writeln!(
            doc,
            "| {:.0} ms | {:.4} | {:.4} | {} / {} | {} / {} |",
            s.network_ms.unwrap(),
            last.old_cumulative,
            last.new_cumulative,
            s.old_wins,
            s.new_wins,
            s.old_losses,
            s.new_losses
        )?;
        println!(
            "{:.0} ms overhead: old profit {:.4}, new {:.4}; wins {}/{}; losing contracts {}/{}",
            s.network_ms.unwrap(),
            last.old_cumulative,
            last.new_cumulative,
            s.old_wins,
            s.new_wins,
            s.old_losses,
            s.new_losses
        );
    }
    doc.push_str(comparison::DESCRIPTION);
    writeln!(
        doc,
        "\n## Backtest on earlier history\n\n![Backtest against earlier public bids](historical-profit.svg)\n\n{} complete older auctions were recovered from the earlier API trace. The\narchived subset preserves proposals, awards, results, and original timestamps.\nBecause historical task parameters were not included in that trace, the replay\njoins the later frozen task pool only when ID, type, budget, deadline, and the\nold recorded result all match. Unmatched records are excluded. The fixture\ndocuments this reconstruction; this is not a full historical tournament.\nEach historical auction is replayed once, in chronological order, starting\nwith empty learning. No later outcome is used for an earlier decision.\n\n| Added delivery time | Old profit | New profit | Old/new wins | Old/new losing contracts |\n|---|---:|---:|---:|---:|",
        old_cases.tasks.len()
    )?;
    for s in &historical_scenarios {
        let last = s.outcomes.last().unwrap();
        writeln!(
            doc,
            "| {:.0} ms | {:.4} | {:.4} | {} / {} | {} / {} |",
            s.network_ms.unwrap(),
            last.old_cumulative,
            last.new_cumulative,
            s.old_wins,
            s.new_wins,
            s.old_losses,
            s.new_losses
        )?;
        println!(
            "Historical {:.0} ms: old profit {:.4}, new {:.4}; wins {}/{}; losing contracts {}/{}",
            s.network_ms.unwrap(),
            last.old_cumulative,
            last.new_cumulative,
            s.old_wins,
            s.new_wins,
            s.old_losses,
            s.new_losses
        );
    }
    doc.push_str(&delivery_doc);
    doc.push_str("\n## Decision latency\n\nMeasured calls to the real `on_cfp` function, with 512 learned observations and\nthe captured public market snapshot. Each task has 25 warmups followed by\n1,000 timed decisions. The table combines samples by task type and uses\nnearest-rank percentiles. Units are **microseconds**. File I/O, networking,\nqueue handling, and task execution are excluded.\n\n| Task | Decisions | p50 | p95 | p99 |\n|---|---:|---:|---:|---:|\n");
    let mut samples: BTreeMap<&str, Vec<u64>> = BTreeMap::new();
    for m in &data.tasks {
        samples
            .entry(&m.task.task_type)
            .or_default()
            .extend(&m.decision_samples_ns);
    }
    let mut csv = String::from("task_type,decisions,p50_us,p95_us,p99_us\n");
    for (kind, samples) in samples {
        let stats = statistics::summarize(&samples)?;
        writeln!(
            doc,
            "| `{kind}` | {} | {:.3} | {:.3} | {:.3} |",
            stats.samples,
            stats.p50_ms * 1000.0,
            stats.p95_ms * 1000.0,
            stats.p99_ms * 1000.0
        )?;
        writeln!(
            csv,
            "{kind},{},{:.3},{:.3},{:.3}",
            stats.samples,
            stats.p50_ms * 1000.0,
            stats.p95_ms * 1000.0,
            stats.p99_ms * 1000.0
        )?;
    }
    fs::write(args.output.join("decision-latency.csv"), csv)?;
    writeln!(
        doc,
        "\nMeasured with `{}` on `{}`. Raw decision samples use `std::time::Instant` in nanoseconds.",
        data.runtime, data.platform
    )?;
    writeln!(
        doc,
        "\n## Provenance and reproduction\n\n- Capture: `{}`, at {}.\n- Base commit: `{}`; measurements completed {}. The measured working-tree\n  sources are identified by per-file SHA-256 hashes in the raw data.\n- [Captured auctions](../../tests/fixtures/practice-market.json).\n- [Earlier historical subset and provenance](../../tests/fixtures/practice-history.json).\n- [Raw timings, calibration, and source hashes](../../tests/benchmarks/results/bidder.json).\n- [Every simulated outcome](replay.json).\n- [Decision latency CSV](decision-latency.csv).\n\n```bash\ncargo run --locked --release --features benchmark-tools --bin bidder-replay\n# Regenerate from the same saved timings:\ncargo run --locked --release --features benchmark-tools --bin bidder-replay -- --render-only\n```",
        data.capture_source, data.capture_time, data.source_commit, data.completed_at
    )?;
    fs::write(args.output.join("README.md"), doc)?;
    Ok(())
}
