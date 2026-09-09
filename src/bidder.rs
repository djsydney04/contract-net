//! Bounded, deterministic learning from actual execution and settlement clocks.
use crate::{Task, strategy::BidContext};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    path::{Path, PathBuf},
};

const HISTORY_LIMIT: usize = 512;
const COLD_DELIVERY_SECONDS: f64 = 0.050;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Observation {
    pub task_key: String,
    pub task_type: String,
    pub baseline_seconds: f64,
    pub local_seconds: f64,
    pub queue_seconds: f64,
    pub manager_seconds: f64,
    pub cost_rate: f64,
    pub cost: f64,
    pub profit: f64,
}

impl Observation {
    fn valid(&self) -> bool {
        [
            self.baseline_seconds,
            self.local_seconds,
            self.queue_seconds,
            self.manager_seconds,
            self.cost_rate,
            self.cost,
        ]
        .iter()
        .all(|v| v.is_finite() && *v >= 0.0)
            && self.profit.is_finite()
            && self.baseline_seconds > 0.0
            && self.local_seconds > 0.0
            && self.manager_seconds >= self.local_seconds
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Learning {
    pub observations: VecDeque<Observation>,
}

#[derive(Clone, Copy, Debug)]
pub struct Forecast {
    pub compute_seconds: f64,
    pub expected_compute_seconds: f64,
    pub delivery_seconds: f64,
    pub samples: usize,
}

pub fn task_key(task: &Task) -> String {
    // IDs are reused by looping rooms. Only exact input content identifies work.
    format!(
        "{:x}",
        Sha256::digest(format!("{}:{}", task.task_type, task.params).as_bytes())
    )
}

pub fn default_state_path(url: &str, name: &str) -> PathBuf {
    let digest = format!("{:x}", Sha256::digest(format!("{url}\n{name}").as_bytes()));
    PathBuf::from("data/runtime").join(format!("bidder-{}.json", &digest[..16]))
}

fn quantile(values: &mut [f64], percent: usize) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[(percent * values.len()).div_ceil(100).saturating_sub(1)])
}

impl Learning {
    pub fn record(&mut self, observation: Observation) {
        if observation.valid() {
            self.observations.push_back(observation);
            while self.observations.len() > HISTORY_LIMIT {
                self.observations.pop_front();
            }
        }
    }

    pub fn forecast(&self, task: &Task, baseline: f64) -> Forecast {
        let key = task_key(task);
        let exact: Vec<_> = self
            .observations
            .iter()
            .filter(|o| o.task_key == key)
            .collect();
        let matching: Vec<_> = if exact.is_empty() && task.task_type != "hash_search" {
            self.observations
                .iter()
                .filter(|o| o.task_type == task.task_type)
                .collect()
        } else {
            exact
        };
        let mut ratios: Vec<_> = matching
            .iter()
            .map(|o| o.local_seconds / o.baseline_seconds)
            .collect();
        let cold_factor = if task.task_type == "hash_search" {
            3.0
        } else {
            1.2
        };
        let factor = quantile(&mut ratios, 95).unwrap_or(cold_factor);
        let expected_factor = quantile(&mut ratios, 50).unwrap_or(1.0);
        // Small samples retain a margin; hash inputs learn their actual search
        // difficulty only after that exact seed/threshold has been executed.
        let compute_seconds =
            (baseline * factor * if matching.len() < 5 { 1.1 } else { 1.05 }).max(0.000_001);
        let mut overhead: Vec<_> = self
            .observations
            .iter()
            .map(|o| {
                let billed = if o.cost_rate > 0.0 {
                    o.cost / o.cost_rate
                } else {
                    o.manager_seconds
                };
                (o.manager_seconds.max(billed) - o.local_seconds - o.queue_seconds).max(0.0)
            })
            .collect();
        let delivery_seconds = quantile(&mut overhead, 95)
            .unwrap_or(COLD_DELIVERY_SECONDS)
            .max(0.005)
            + 0.005;
        Forecast {
            compute_seconds,
            expected_compute_seconds: (baseline * expected_factor * 1.05).max(0.000_001),
            delivery_seconds,
            samples: matching.len(),
        }
    }

    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let file: Saved = serde_json::from_slice(&fs::read(path)?)?;
        ensure!(file.version == 1, "unsupported bidder state version");
        let mut learning = Self::default();
        for observation in file.learning.observations {
            learning.record(observation);
        }
        Ok(learning)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let temporary = path.with_extension("json.tmp");
        fs::write(
            &temporary,
            serde_json::to_vec_pretty(&Saved {
                version: 1,
                learning: self.clone(),
            })?,
        )?;
        fs::rename(&temporary, path)
            .with_context(|| format!("saving bidder state to {}", path.display()))
    }
}

#[derive(Deserialize, Serialize)]
struct Saved {
    version: u32,
    learning: Learning,
}

pub fn forecast(task: &Task, context: &BidContext<'_>) -> Forecast {
    context.learning.forecast(task, context.estimate(task))
}
