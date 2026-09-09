use crate::fixtures::reference_cases;
use anyhow::{Result, ensure};
use contractnet::{MyContractor, Strategy, Task};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    path::Path,
    time::Instant,
};

/// Verify pinned Python answers; append timings only after every check passes.
pub fn verify(log: Option<&Path>, all: bool) -> Result<usize> {
    let cases = reference_cases();
    let count = if all { cases.len() } else { 5 };
    let mut lines = Vec::new();
    for case in cases.iter().take(count) {
        let start = Instant::now();
        let task = Task {
            task_id: 0,
            task_type: case.task_type.clone(),
            params: case.params.clone(),
            budget: 999.0,
            deadline_s: 999.0,
            bid_window_ms: 0,
            attempt: 1,
        };
        let actual = MyContractor.execute(&task)?;
        let elapsed = start.elapsed().as_secs_f64();
        ensure!(
            actual.to_string() == case.expected,
            "{} mismatch: got {actual}, expected {} (params {})",
            case.task_type,
            case.expected,
            case.params
        );
        let line = format!("  [ok  ] {:<16} {elapsed:8.6}s  {actual}", case.task_type);
        println!("{line}");
        lines.push(line);
    }
    if let Some(path) = log {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let stamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
        let profile = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };
        let block = format!(
            "=== {stamp} ===\nrust {profile} ({count} Python reference comparisons):\n{}\n\n",
            lines.join("\n")
        );
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)?
            .write_all(block.as_bytes())?;
        println!("\nAppended timings to {}", path.display());
    }
    println!("\nAll {count} checks passed.");
    Ok(count)
}
