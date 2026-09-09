use crate::{
    Bid, Rules, Settlement, Task,
    benchmark::Rates,
    bidder::{self, Learning},
    market::{self, Snapshot},
    tasks::run_task,
};
use anyhow::Result;
use num_bigint::BigUint;

pub struct BidContext<'a> {
    pub rules: &'a Rules,
    pub rates: &'a Rates,
    pub history: &'a [Settlement],
    pub queue_seconds: f64,
    pub learning: &'a Learning,
    pub market: Option<&'a Snapshot>,
    pub name: &'a str,
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
    fn compute_reserve(&self, task: &Task, context: &BidContext<'_>) -> f64 {
        context.estimate(task)
    }
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
    fn compute_reserve(&self, task: &Task, context: &BidContext<'_>) -> f64 {
        bidder::forecast(task, context).compute_seconds
    }

    fn on_cfp(&self, task: &Task, context: &BidContext<'_>) -> Option<Bid> {
        let forecast = bidder::forecast(task, context);
        let finish_in =
            context.queue_seconds + forecast.expected_compute_seconds + forecast.delivery_seconds;
        let reserved_finish =
            context.queue_seconds + forecast.compute_seconds + forecast.delivery_seconds;
        if !finish_in.is_finite()
            || finish_in <= 0.0
            || !context.rules.cost_rate.is_finite()
            || context.rules.cost_rate < 0.0
            || !task.budget.is_finite()
            || !task.deadline_s.is_finite()
            || !reserved_finish.is_finite()
            || reserved_finish > task.deadline_s * 0.98
        {
            return None;
        }
        // The manager charges its award-to-delivery clock, including queue and
        // network time. Cover that conservative forecast before seeking wins.
        let cost = reserved_finish * context.rules.cost_rate;
        let floor = (cost * 1.2 + 0.01 * context.rules.cost_rate.max(0.1)).max(0.0001);
        let time_score = market::score(context.rules, 0.0, finish_in)?;
        let ceiling = context
            .market
            .and_then(|m| m.competing_score(task, context.rules, context.name))
            .map(|score| score * 0.98 - time_score)
            .unwrap_or_else(|| (cost * 1.6).max(task.budget * 0.08))
            .min(task.budget * 0.98);
        // Fixed ticks and tie margin make decisions repeatable for identical
        // inputs/observations. Never undercut below the predicted profit floor.
        let price = (ceiling * 10_000.0).floor() / 10_000.0;
        if !price.is_finite() || price < floor {
            return None;
        }
        Some(Bid {
            price,
            est_seconds: finish_in,
        })
    }
}
