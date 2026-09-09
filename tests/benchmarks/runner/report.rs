use super::{
    data::{Dataset, History, write_json},
    statistics::{Percentiles, summarize},
};
use anyhow::Result;
use serde::Serialize;
use std::{fmt::Write as _, fs, path::Path};

#[derive(Serialize)]
pub struct Summary {
    pub task_type: String,
    pub current: Percentiles,
    pub historical_ms: f64,
    pub historical_ratio: Option<f64>,
}

pub fn summaries(current: &Dataset, history: &History) -> Result<Vec<Summary>> {
    current
        .tasks
        .iter()
        .map(|task| {
            let stats = summarize(&task.samples_ns)?;
            let old = history
                .tasks
                .iter()
                .find(|h| h.task_type == task.task_type)
                .ok_or_else(|| {
                    anyhow::anyhow!("missing historical timing for {}", task.task_type)
                })?;
            let ratio = (old.elapsed_ms > 0.0).then_some(old.elapsed_ms / stats.p50_ms);
            Ok(Summary {
                task_type: task.task_type.clone(),
                current: stats,
                historical_ms: old.elapsed_ms,
                historical_ratio: ratio,
            })
        })
        .collect()
}

pub fn write(output: &Path, current: &Dataset, history: &History, rows: &[Summary]) -> Result<()> {
    fs::create_dir_all(output)?;
    write_json(
        &output.join("summary.json"),
        &serde_json::json!({
            "schema_version":1, "generated_at":chrono::Utc::now().to_rfc3339(),
            "quantile_method":"nearest rank: sorted[ceil(p * n) - 1]; no outlier removal",
            "source_commit":current.source_commit, "units":"milliseconds",
            "historical_comparison":history, "tasks":rows,
        }),
    )?;
    let mut csv = String::from(
        "task_type,samples,p50_ms,p95_ms,p99_ms,min_ms,max_ms,historical_single_ms,historical_to_current_p50_ratio\n",
    );
    for row in rows {
        let s = &row.current;
        writeln!(
            csv,
            "{},{},{:.6},{:.6},{:.6},{:.6},{:.6},{:.3},{}",
            row.task_type,
            s.samples,
            s.p50_ms,
            s.p95_ms,
            s.p99_ms,
            s.min_ms,
            s.max_ms,
            row.historical_ms,
            row.historical_ratio
                .map(|v| format!("{v:.6}"))
                .unwrap_or_default()
        )?;
    }
    fs::write(output.join("summary.csv"), csv)?;
    let mut doc = format!(
        "# Rust task latency\n\nMeasured **{} Rust executions per task** after **{} warmups per task**, using\nthe five fixed golden workloads. Benchmarking, statistics, and graph generation\nrun as a native Rust application. Every result was checked against its expected\ninteger answer.\n\n![Rust p50, p95, and p99 latency](latency-percentiles.svg)\n\n## Measured Rust percentiles\n\nAll values are milliseconds. Lower is better. These measure `execute(task)`\ninside the Rust process; networking, queue time, process startup, caller-side\ntask construction, serialization, and answer validation are excluded.\n\n| Task | p50 | p95 | p99 |\n|---|---:|---:|---:|\n",
        current.samples_per_task, current.warmup_iterations
    );
    for row in rows {
        writeln!(
            doc,
            "| `{}` | {:.4} | {:.4} | {:.4} |",
            row.task_type, row.current.p50_ms, row.current.p95_ms, row.current.p99_ms
        )?;
    }
    doc.push_str("\n## Historical comparison\n\n![Approximate comparison against recorded pre-migration timings](historical-comparison.svg)\n\nThis comparison reads the **existing optimized-executor verification log**.\nIt does not run the historical implementation. Each old value is a single\nobservation rounded to 1 ms, not a percentile or a controlled fresh baseline.\nThe ratio is the old recorded value divided by the new Rust p50. Treat it as\nan approximate historical comparison, not a statistically established speedup.\n\nThe prime-count entry rounded to zero in that log, so its ratio is unavailable.\nNo historical p95 or p99 values are inferred from the old records.\n\n| Task | Recorded old timing (ms) | New Rust p50 (ms) | Approximate ratio |\n|---|---:|---:|---:|\n");
    for row in rows {
        writeln!(
            doc,
            "| `{}` | {:.0} | {:.4} | {} |",
            row.task_type,
            row.historical_ms,
            row.current.p50_ms,
            row.historical_ratio
                .map(|r| format!("{r:.2}x"))
                .unwrap_or_else(|| "Unavailable: old timing rounded to zero".into())
        )?;
    }
    writeln!(
        doc,
        "\nHistorical source: `{}`, recorded **{}**.\n",
        history.source_path, history.recorded_at
    )?;
    writeln!(
        doc,
        "## Method and provenance\n\n- CPU: **{}**, {} logical CPUs; `{}` / `{}`.\n- Rust source commit: `{}`; source tree: `{}`.\n- Runtime: `{}`. Build: `{}`.\n- Measurement window: {} to {}.\n- Clock: `{}`.\n- Sampling: {}.\n- Percentiles use nearest rank: sorted sample at `ceil(p × n) - 1`.\n- No outliers were removed. The p99 is an empirical percentile, not a maximum\n  or a latency guarantee. These observations describe one local run.\n- Fixed seeded inputs hold computation constant. `hash_search` repeats the\n  same nonce search, so its tail reflects timing variation rather than\n  differences in search difficulty across seeds.\n",
        current.host.cpu,
        current.host.logical_cpus,
        current.host.os,
        current.host.arch,
        current.source_commit,
        current.source_tree,
        current.runtime,
        current.profile,
        current.started_at,
        current.completed_at,
        current.clock,
        current.order
    )?;
    doc.push_str(
        "### Workloads\n\n| Task | Exact parameters | Expected result |\n|---|---|---:|\n",
    );
    for task in &current.tasks {
        writeln!(
            doc,
            "| `{}` | `{}` | `{}` |",
            task.task_type, task.params, task.expected
        )?;
    }
    doc.push_str("\n## Data and reproduction\n\n- [Raw Rust samples](../tests/benchmarks/results/rust.json): every nanosecond\n  observation in measurement order, plus run metadata.\n- [summary.csv](summary.csv) and [summary.json](summary.json): derived values\n  and historical comparison provenance.\n- Graphs are exported as both SVG and PNG.\n- [Runner source](../tests/benchmarks/runner) and\n  [methodology](../tests/benchmarks/results/README.md).\n\nRun the Rust benchmark:\n\n```bash\ncargo run --locked --release --features benchmark-tools --bin benchmark -- \\\n  --samples 1000 --warmup 25\n```\n\nRegenerate graphs from saved Rust measurements:\n\n```bash\ncargo run --locked --release --features benchmark-tools --bin benchmark -- --render-only\n```\n");
    fs::write(output.join("README.md"), doc)?;
    Ok(())
}
