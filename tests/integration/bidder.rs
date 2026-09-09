use contractnet::{
    BidContext, MyContractor, Rules, Strategy, Task,
    benchmark::Rates,
    bidder::{Learning, Observation, task_key},
    market::{Auction, MarketBid, Snapshot, score},
};
use serde_json::json;

struct Fixture {
    task: Task,
    rules: Rules,
    rates: Rates,
    learning: Learning,
    market: Option<Snapshot>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            task: Task {
                task_id: 1,
                task_type: "monte_carlo_pi".into(),
                params: json!({"seed":1,"samples":1000}),
                budget: 10.0,
                deadline_s: 5.0,
                bid_window_ms: 5000,
                attempt: 1,
            },
            rules: Rules::default(),
            rates: Rates::from([("monte_carlo_pi".into(), 1000.0)]),
            learning: Learning::default(),
            market: None,
        }
    }
    fn context(&self, queue_seconds: f64) -> BidContext<'_> {
        BidContext {
            rules: &self.rules,
            rates: &self.rates,
            history: &[],
            queue_seconds,
            learning: &self.learning,
            market: self.market.as_ref(),
            name: "Us",
        }
    }
    fn competitor(&mut self, price: f64, seconds: f64) {
        self.market = Some(Snapshot {
            now: 1000,
            config: self.rules.clone(),
            active: vec![Auction {
                task: self.task.clone(),
                state: "bidding".into(),
                bids_close_at: Some(6000),
            }],
            live_bids: vec![MarketBid {
                task_id: 1,
                agent: "Other".into(),
                price,
                est_seconds: seconds,
                outcome: None,
            }],
        });
    }
    fn observe(&mut self, local: f64, manager: f64) {
        self.learning.record(Observation {
            task_key: task_key(&self.task),
            task_type: self.task.task_type.clone(),
            baseline_seconds: self.context(0.0).estimate(&self.task),
            local_seconds: local,
            queue_seconds: 0.0,
            manager_seconds: manager,
            cost_rate: 1.0,
            cost: manager,
            profit: 0.1,
        });
    }
}

#[test]
fn small_tasks_cover_delivery_cost_instead_of_repeating_compute_only_losses() {
    let mut f = Fixture::new();
    f.rates.insert("monte_carlo_pi".into(), 1_000_000.0);
    f.observe(0.001, 0.041);
    f.competitor(0.10, 0.10);
    let context = f.context(0.0);
    let bid = MyContractor.on_cfp(&f.task, &context).unwrap();
    let old_price = context.estimate(&f.task) * f.rules.cost_rate * 1.6;
    assert!(
        old_price < 0.041,
        "old bidder loses money on this settlement"
    );
    assert!(bid.price > 0.041, "new price covers measured manager cost");
    assert!(bid.est_seconds >= 0.041);
    assert!(
        score(&f.rules, bid.price, bid.est_seconds).unwrap() < 0.30,
        "still beats the public competitor score"
    );
}

#[test]
fn decisions_are_repeatable_and_take_profitable_room_in_competing_scores() {
    let mut f = Fixture::new();
    f.competitor(5.0, 2.0);
    let first = MyContractor.on_cfp(&f.task, &f.context(0.0)).unwrap();
    for _ in 0..100 {
        assert_eq!(Some(first), MyContractor.on_cfp(&f.task, &f.context(0.0)));
    }
    assert!(
        first.price > 1.6,
        "fast execution should not leave the entire score advantage unpriced"
    );
    assert!(score(&f.rules, first.price, first.est_seconds).unwrap() < 9.0);
    assert!(first.price <= f.task.budget);
}

#[test]
fn refuses_competitive_prices_that_cannot_cover_cost() {
    let mut f = Fixture::new();
    f.competitor(0.01, 0.01);
    assert!(MyContractor.on_cfp(&f.task, &f.context(0.0)).is_none());
}

#[test]
fn queued_time_is_billed_and_tail_estimates_control_deadline_admission() {
    let mut f = Fixture::new();
    f.rules.cost_rate = 3.0;
    assert!(MyContractor.on_cfp(&f.task, &f.context(0.0)).is_some());
    assert!(
        MyContractor.on_cfp(&f.task, &f.context(2.0)).is_none(),
        "queue cost exceeds the budget with a profit margin"
    );
    f.task.budget = 100.0;
    f.task.deadline_s = 1.2;
    assert!(
        MyContractor.on_cfp(&f.task, &f.context(0.0)).is_none(),
        "mean fits but conservative completion does not"
    );
}

#[test]
fn settlement_learning_changes_estimates_and_is_keyed_by_content_not_reused_ids() {
    let mut f = Fixture::new();
    let before = f.learning.forecast(&f.task, 1.0);
    f.observe(2.0, 2.075);
    let after = f.learning.forecast(&f.task, 1.0);
    assert!(after.compute_seconds > before.compute_seconds);
    assert!(after.delivery_seconds > before.delivery_seconds);
    let key = task_key(&f.task);
    f.task.task_id = 999;
    assert_eq!(task_key(&f.task), key);
    assert_eq!(f.learning.forecast(&f.task, 1.0).samples, 1);
}

#[test]
fn hash_search_learns_exact_seed_difficulty_and_keeps_an_unseen_seed_tail_reserve() {
    let mut f = Fixture::new();
    f.task.task_type = "hash_search".into();
    f.task.params = json!({"seed":"known","threshold":4096});
    f.rates.insert("hash_search".into(), 5_000_000.0);
    f.observe(0.01, 0.05);
    let baseline = f.context(0.0).estimate(&f.task);
    let known = f.learning.forecast(&f.task, baseline);
    f.task.params["seed"] = "unseen".into();
    let unseen = f.learning.forecast(&f.task, baseline);
    assert_eq!(unseen.samples, 0);
    assert!(unseen.compute_seconds > known.compute_seconds * 10.0);
    assert!(unseen.compute_seconds > unseen.expected_compute_seconds * 2.0);
}

#[test]
fn stale_wrong_input_invalid_and_own_bids_do_not_drive_prices() {
    let mut f = Fixture::new();
    let without = MyContractor.on_cfp(&f.task, &f.context(0.0));
    for variant in 0..5 {
        f.competitor(0.001, 0.001);
        let market = f.market.as_mut().unwrap();
        match variant {
            0 => market.active[0].bids_close_at = Some(1100),
            1 => market.active[0].task.params["seed"] = 999.into(),
            2 => market.live_bids[0].outcome = Some("invalid".into()),
            3 => market.live_bids[0].agent = "Us".into(),
            _ => market.config.time_weight = 9.0,
        }
        assert_eq!(MyContractor.on_cfp(&f.task, &f.context(0.0)), without);
    }
}

#[test]
fn price_only_policy_and_live_cost_changes_are_respected() {
    let mut f = Fixture::new();
    f.rules.award_policy = "lowest_price".into();
    f.competitor(4.0, 4.0);
    let bid = MyContractor.on_cfp(&f.task, &f.context(0.0)).unwrap();
    assert!(bid.price < 4.0);
    f.rules.cost_rate = 20.0;
    assert!(MyContractor.on_cfp(&f.task, &f.context(0.0)).is_none());
    f.rules.award_policy = "unknown".into();
    assert!(MyContractor.on_cfp(&f.task, &f.context(0.0)).is_none());
}

#[test]
fn market_json_preserves_decimal_fields_and_large_integer_params() {
    let raw = r#"{"now":1000,"config":{"penalty_rate":0.5},"active":[{"task_id":1,"task_type":"matmul_mod","params":{"seed":1,"n":4,"mod":1267650600228229401496703205376},"budget":1.25,"deadline_s":2.5,"state":"bidding","bids_close_at":6000}],"live_bids":[]}"#;
    let snapshot = Snapshot::parse(raw).unwrap();
    assert_eq!(
        snapshot.active[0].task.params["mod"].to_string(),
        "1267650600228229401496703205376"
    );
    assert_eq!(snapshot.active[0].task.budget, 1.25);
    let round_trip = Snapshot::parse(&serde_json::to_string(&snapshot).unwrap()).unwrap();
    assert_eq!(
        round_trip.active[0].task.params,
        snapshot.active[0].task.params
    );
}

#[test]
fn malformed_observations_and_invalid_task_inputs_cannot_make_valid_bids() {
    let mut f = Fixture::new();
    f.observe(f64::NAN, 1.0);
    assert!(f.learning.observations.is_empty());
    f.task.budget = f64::NAN;
    assert!(MyContractor.on_cfp(&f.task, &f.context(0.0)).is_none());
    f.task.budget = 10.0;
    f.rates.clear();
    assert!(MyContractor.on_cfp(&f.task, &f.context(0.0)).is_none());
}
