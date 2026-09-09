use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Percentiles {
    pub samples: usize,
    pub min_ms: f64,
    pub p50_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

/// Empirical nearest-rank quantiles: sorted[ceil(p * n) - 1]. No outlier removal.
pub fn summarize(samples_ns: &[u64]) -> Result<Percentiles> {
    ensure!(
        !samples_ns.is_empty(),
        "cannot summarize an empty sample set"
    );
    ensure!(
        samples_ns.iter().all(|&ns| ns > 0),
        "latencies must be positive"
    );
    let mut sorted = samples_ns.to_vec();
    sorted.sort_unstable();
    let percentile = |percent: usize| {
        let rank = (percent * sorted.len()).div_ceil(100);
        sorted[rank.saturating_sub(1)] as f64 / 1_000_000.0
    };
    Ok(Percentiles {
        samples: sorted.len(),
        min_ms: sorted[0] as f64 / 1_000_000.0,
        p50_ms: percentile(50),
        p95_ms: percentile(95),
        p99_ms: percentile(99),
        max_ms: sorted[sorted.len() - 1] as f64 / 1_000_000.0,
    })
}

#[cfg(test)]
mod tests {
    use super::summarize;

    #[test]
    fn nearest_rank_percentiles_preserve_tail_outliers() {
        let mut samples: Vec<u64> = (1..=1000).map(|n| n * 1_000_000).collect();
        samples[999] = 50_000_000_000;
        samples.reverse();
        let stats = summarize(&samples).unwrap();
        assert_eq!(stats.p50_ms, 500.0);
        assert_eq!(stats.p95_ms, 950.0);
        assert_eq!(stats.p99_ms, 990.0);
        assert_eq!(stats.max_ms, 50_000.0);
    }

    #[test]
    fn uneven_samples_use_ceiling_ranks_and_reject_invalid_values() {
        let stats = summarize(&[5_000_000, 1_000_000, 3_000_000]).unwrap();
        assert_eq!(stats.p50_ms, 3.0);
        assert_eq!(stats.p95_ms, 5.0);
        assert_eq!(stats.p99_ms, 5.0);
        assert!(summarize(&[]).is_err());
        assert!(summarize(&[0]).is_err());
    }
}
