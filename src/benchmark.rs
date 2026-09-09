//! Calibration measures the same native algorithms used to fulfill awards.
use crate::tasks::{TASK_TYPES, count, hash_attempt, hash_prefix, integer, run_task};
use anyhow::{Context, Result, bail, ensure};
use num_traits::ToPrimitive;
use serde_json::{Value, json};
use std::{collections::BTreeMap, time::Instant};

pub type Rates = BTreeMap<String, f64>;
pub const HASH_CALIBRATION_ROUNDS: u64 = 150_000;

pub fn work_units(task_type: &str, params: &Value) -> Result<f64> {
    let work = match task_type {
        "monte_carlo_pi" => count(params, "samples")? as f64,
        "prime_count" => {
            let lo = integer(params, "lo")?
                .to_f64()
                .context("invalid lower bound")?
                .max(2.0);
            let hi = integer(params, "hi")?
                .to_f64()
                .context("invalid upper bound")?;
            // Segmented sieve: interval marking plus the base sieve.
            if hi <= lo { 0.0 } else { hi - lo + hi.sqrt() }
        }
        "hash_search" => {
            let threshold = integer(params, "threshold")?
                .to_f64()
                .context("invalid threshold")?;
            ensure!(threshold > 0.0, "hash threshold must be positive");
            (4_294_967_296.0 / threshold).max(1.0)
        }
        "sort_checksum" => {
            let n = count(params, "n")? as f64;
            n * n.max(2.0).log2()
        }
        "matmul_mod" => {
            let n = count(params, "n")? as f64;
            let modulus = integer(params, "mod")?;
            ensure!(modulus > 0.into(), "matrix modulus must be positive");
            // Native executor uses the O(n²) checksum identity.
            n * n * (modulus.bits().div_ceil(32).max(1) as f64)
        }
        _ => bail!("unknown task type: {task_type}"),
    };
    ensure!(
        work.is_finite() && work >= 0.0,
        "invalid task work estimate"
    );
    Ok(work)
}

pub fn calibrate(verbose: bool) -> Result<Rates> {
    let mut rates = Rates::new();
    for kind in TASK_TYPES {
        let params = match kind {
            "monte_carlo_pi" => json!({"seed":1,"samples":200_000}),
            "prime_count" => json!({"lo":2_000_000,"hi":2_025_000}),
            "sort_checksum" => json!({"seed":1,"n":200_000}),
            "matmul_mod" => json!({"seed":1,"n":70,"mod":1_000_003}),
            _ => Value::Null,
        };
        let start = Instant::now();
        let units = if kind == "hash_search" {
            let prefix = hash_prefix("calibrate");
            for nonce in 0..HASH_CALIBRATION_ROUNDS {
                std::hint::black_box(hash_attempt(&prefix, nonce));
            }
            HASH_CALIBRATION_ROUNDS as f64
        } else {
            std::hint::black_box(run_task(kind, &params)?);
            work_units(kind, &params)?
        };
        let elapsed = start.elapsed().as_secs_f64().max(1e-6);
        rates.insert(kind.into(), units / elapsed);
        if verbose {
            println!(
                "  {kind:<16} {elapsed:6.3}s  {:.0} units/s",
                units / elapsed
            );
        }
    }
    Ok(rates)
}

pub fn overall_score(rates: &Rates) -> f64 {
    let values: Vec<f64> = rates
        .values()
        .copied()
        .filter(|r| r.is_finite() && *r > 0.0)
        .collect();
    if values.is_empty() {
        return 0.0;
    }
    (values.iter().map(|v| v.ln()).sum::<f64>() / values.len() as f64).exp()
}
