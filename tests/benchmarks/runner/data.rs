use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path, process::Command};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Host {
    pub os: String,
    pub arch: String,
    pub cpu: String,
    #[serde(default)]
    pub cpu_metadata_source: String,
    pub logical_cpus: usize,
}

impl Host {
    pub fn current() -> Self {
        let model = command("sysctl", &["-n", "machdep.cpu.brand_string"]);
        Self {
            os: command("uname", &["-sr"]).unwrap_or_else(|| std::env::consts::OS.into()),
            arch: std::env::consts::ARCH.into(),
            cpu_metadata_source: if model.is_some() {
                "sysctl"
            } else {
                "Architecture fallback; CPU model detection unavailable"
            }
            .into(),
            cpu: model.unwrap_or_else(|| std::env::consts::ARCH.into()),
            logical_cpus: std::thread::available_parallelism()
                .map(usize::from)
                .unwrap_or(1),
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct TaskSamples {
    pub task_type: String,
    pub params: Value,
    pub expected: String,
    pub samples_ns: Vec<u64>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Dataset {
    pub schema_version: u32,
    pub implementation: String,
    pub source_commit: String,
    pub source_tree: String,
    pub runtime: String,
    pub profile: String,
    pub started_at: String,
    pub completed_at: String,
    pub host: Host,
    pub samples_per_task: usize,
    pub warmup_iterations: usize,
    pub clock: String,
    pub scope: String,
    pub order: String,
    pub tasks: Vec<TaskSamples>,
}

pub fn command(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

pub fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    let mut contents = serde_json::to_string_pretty(value)?;
    contents.push('\n');
    fs::write(path, contents).with_context(|| format!("writing {}", path.display()))
}

pub fn read_dataset(path: &Path) -> Result<Dataset> {
    let dataset: Dataset = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(dataset.schema_version == 1, "unsupported benchmark schema");
    ensure!(
        dataset.samples_per_task >= 100,
        "at least 100 observations are required for p99"
    );
    ensure!(
        dataset.tasks.len() == 5,
        "benchmark must contain the five golden tasks"
    );
    chrono::DateTime::parse_from_rfc3339(&dataset.completed_at)?;
    for task in &dataset.tasks {
        ensure!(
            task.samples_ns.len() == dataset.samples_per_task,
            "incomplete samples for {}",
            task.task_type
        );
        super::statistics::summarize(&task.samples_ns)?;
    }
    Ok(dataset)
}

#[derive(Debug, Serialize)]
pub struct HistoricalTiming {
    pub task_type: String,
    pub elapsed_ms: f64,
}

#[derive(Debug, Serialize)]
pub struct History {
    pub source_path: String,
    pub recorded_at: String,
    pub resolution_ms: f64,
    pub description: String,
    pub tasks: Vec<HistoricalTiming>,
}

/// Read existing timings as recorded. They are individual rounded observations,
/// never fabricated percentile samples or measurements from this run.
pub fn read_history(path: &Path) -> Result<History> {
    let text = fs::read_to_string(path)?;
    let mut stamp = String::new();
    let mut current: Option<History> = None;
    let mut latest = None;
    for line in text.lines() {
        if line.starts_with("=== ") {
            stamp = line.trim_matches('=').trim().to_owned();
            current = None;
        } else if line == "custom execute() vs reference:" {
            current = Some(History {
                source_path: path.display().to_string(), recorded_at: stamp.clone(),
                resolution_ms: 1.0,
                description: "Existing optimized executor verification timings before the Rust migration; one recorded observation per task, rounded to 1 ms. No historical executor was run for this report.".into(),
                tasks: Vec::new(),
            });
        } else if let Some(history) = &mut current
            && line.trim_start().starts_with("[ok")
            && let Some((_, body)) = line.split_once(']')
            && let Some((_, timing)) = body.split_once("yours ")
        {
            let task_type = body
                .split_whitespace()
                .next()
                .context("missing historical task name")?;
            let seconds = timing
                .split_whitespace()
                .next()
                .context("missing historical timing")?
                .trim_end_matches('s')
                .parse::<f64>()?;
            ensure!(
                seconds.is_finite() && seconds >= 0.0,
                "invalid historical timing"
            );
            history.tasks.push(HistoricalTiming {
                task_type: task_type.into(),
                elapsed_ms: seconds * 1000.0,
            });
            if history.tasks.len() == 5 {
                latest = current.take();
            }
        }
    }
    let history = latest.context("verification log has no complete historical executor timings")?;
    for kind in contractnet::tasks::TASK_TYPES {
        ensure!(
            history.tasks.iter().filter(|t| t.task_type == kind).count() == 1,
            "historical log must contain exactly one timing for {kind}"
        );
    }
    Ok(history)
}
