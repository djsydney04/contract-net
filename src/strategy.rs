use crate::{Bid, Rules, Settlement, Task, benchmark::Rates, tasks::run_task};
use anyhow::Result;
use num_bigint::BigUint;

pub struct BidContext<'a> {
    pub rules: &'a Rules,
    pub rates: &'a Rates,
    pub history: &'a [Settlement],
    pub queue_seconds: f64,
}

impl BidContext<'_> {
    pub fn estimate(&self, task: &Task) -> f64 {
        let rate = self.rates.get(&task.task_type).copied().unwrap_or(0.0);
        if !rate.is_finite() || rate <= 0.0 {
            return f64::INFINITY;
        }
        task.work().map(|work| work / rate).unwrap_or(f64::INFINITY)
    }

    pub fn profit(&self) -> f64 {
        self.history.iter().map(|s| s.profit).sum()
    }
}

/// Bidding hooks run on the async event loop; execute runs on a blocking worker.
/// Interior state can use a Mutex if a custom strategy learns from settlements.
pub trait Strategy: Send + Sync + 'static {
    fn on_cfp(&self, task: &Task, context: &BidContext<'_>) -> Option<Bid>;
    fn execute(&self, task: &Task) -> Result<BigUint> {
        run_task(&task.task_type, &task.params)
    }
    fn on_registered(&self, _context: &BidContext<'_>) {}
    fn on_reject(&self, _task_id: u64, _winner: Option<&str>, _price: Option<f64>) {}
    fn on_settled(&self, _settlement: &Settlement) {}
    fn on_bid_invalid(&self, _task_id: u64, _reason: &str) {}
}

#[derive(Default)]
pub struct MyContractor;

impl Strategy for MyContractor {
    fn on_cfp(&self, task: &Task, context: &BidContext<'_>) -> Option<Bid> {
        let compute_seconds = context.estimate(task);
        let finish_in = context.queue_seconds + compute_seconds;
        let price = compute_seconds * context.rules.cost_rate * 1.6;
        if !finish_in.is_finite()
            || !price.is_finite()
            || price < 0.0
            || !task.budget.is_finite()
            || !task.deadline_s.is_finite()
            || finish_in > task.deadline_s
            || price > task.budget
        {
            return None;
        }
        Some(Bid {
            price,
            est_seconds: finish_in,
        })
    }
}
