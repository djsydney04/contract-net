use anyhow::{Result, ensure};
use clap::Parser;
use contractnet::{MyContractor, Strategy, Task};
use std::{hint::black_box, path::PathBuf, time::Instant};

mod data;
#[path = "../../support/fixtures.rs"]
mod fixtures;
mod graphs;
mod report;
mod statistics;

#[derive(Parser)]
#[command(about = "Measure repeated task latency and generate percentile and improvement graphs")]
struct Args {
    /// Timed executions per task, after warmup.
    #[arg(long, default_value_t = 1000)]
    samples: usize,
    /// Untimed executions per task before sampling.
    #[arg(long, default_value_t = 25)]
    warmup: usize,
    /// Current run's raw nanosecond samples and provenance.
    #[arg(long, default_value = "tests/benchmarks/results/rust.json")]
    data: PathBuf,
    /// Existing verification log used only for a labeled historical comparison.
    #[arg(long, default_value = "tests/benchmarks/verify_benchmarks.log")]
    history: PathBuf,
    /// Directory for SVG/PNG graphs and the report.
    #[arg(long, default_value = "graph")]
    output: PathBuf,
    /// Regenerate graphs from saved data without timing new executions.
    #[arg(long)]
    render_only: bool,
}

fn measure(samples: usize, warmup: usize) -> Result<data::Dataset> {
    ensure!(
        !cfg!(debug_assertions),
        "benchmark measurements require --release"
    );
    ensure!(samples >= 100, "use at least 100 samples per task for p99");
    ensure!(warmup > 0, "use at least one warmup per task");
    let started_at = chrono::Utc::now().to_rfc3339();
    let host = data::Host::current();
    let runtime = data::command("rustc", &["--version"]).unwrap_or_default();
    let source_commit = data::command("git", &["rev-parse", "HEAD"]).unwrap_or_default();
    let source_tree = data::command("git", &["rev-parse", "HEAD:src"]).unwrap_or_default();
    let fixtures = fixtures::reference_cases();
    let fixtures = &fixtures[..5];
    let tasks: Vec<Task> = fixtures
        .iter()
        .enumerate()
        .map(|(id, case)| Task {
            task_id: id as u64,
            task_type: case.task_type.clone(),
            params: case.params.clone(),
            budget: 999.0,
            deadline_s: 999.0,
            bid_window_ms: 0,
            attempt: 1,
        })
        .collect();
    let mut measurements: Vec<data::TaskSamples> = fixtures
        .iter()
        .map(|case| data::TaskSamples {
            task_type: case.task_type.clone(),
            params: case.params.clone(),
            expected: case.expected.clone(),
            samples_ns: Vec::with_capacity(samples),
        })
        .collect();
    println!(
        "Warming each task {warmup} times, then collecting {samples} individual executions per task."
    );
    for round in 0..warmup + samples {
        for offset in 0..tasks.len() {
            let index = (round + offset) % tasks.len();
            let task = &tasks[index];
            let start = Instant::now();
            let answer = black_box(MyContractor.execute(black_box(task)))?;
            let elapsed_ns = u64::try_from(start.elapsed().as_nanos())?;
            // Compare after stopping the clock: string formatting is not compute time.
            ensure!(
                answer.to_string() == fixtures[index].expected,
                "{} returned a wrong answer",
                task.task_type
            );
            if round >= warmup {
                measurements[index].samples_ns.push(elapsed_ns);
            }
        }
        if round >= warmup && (round + 1 - warmup).is_multiple_of(100) {
            println!(
                "  collected {} / {samples} samples per task",
                round + 1 - warmup
            );
        }
    }
    Ok(data::Dataset {
        schema_version: 1, implementation: "Rust".into(), source_commit, source_tree, runtime,
        profile: "release; thin LTO; one codegen unit".into(), started_at,
        completed_at: chrono::Utc::now().to_rfc3339(), host,
        samples_per_task: samples, warmup_iterations: warmup, clock: "std::time::Instant; nanoseconds".into(),
        scope: "One in-process execute(task) call; includes parameter parsing, RNG initialization, allocation and computation; excludes task construction, result validation, serialization, networking and queueing".into(),
        order: "Serial executions; rotate task order every round; fixed golden inputs; warmup retained only as untimed work; no outlier removal".into(),
        tasks: measurements,
    })
}

fn main() -> Result<()> {
    let args = Args::parse();
    let history = data::read_history(&args.history)?;
    let current = if args.render_only {
        data::read_dataset(&args.data)?
    } else {
        let dataset = measure(args.samples, args.warmup)?;
        data::write_json(&args.data, &dataset)?;
        println!("Saved raw samples to {}", args.data.display());
        dataset
    };
    let summaries = report::summaries(&current, &history)?;
    graphs::render(&args.output, &current, &summaries)?;
    report::write(&args.output, &current, &history, &summaries)?;
    println!(
        "\nGenerated graphs and report in {}/",
        args.output.display()
    );
    for row in summaries {
        println!(
            "{:<16} p50 {:9.4} ms  p95 {:9.4} ms  p99 {:9.4} ms{}",
            row.task_type,
            row.current.p50_ms,
            row.current.p95_ms,
            row.current.p99_ms,
            row.historical_ratio
                .map(|s| format!("  {s:.2}x vs rounded historical timing"))
                .unwrap_or_default()
        );
    }
    Ok(())
}
