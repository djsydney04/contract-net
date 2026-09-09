use contractnet::{BidContext, MyContractor, Rules, Strategy, Task, benchmark::Rates};
use serde_json::json;

#[test]
fn bids_use_live_cost_rate_and_charge_for_queue_time() {
    let rules = Rules {
        cost_rate: 3.0,
        ..Rules::default()
    };
    let rates = Rates::from([("monte_carlo_pi".into(), 1000.0)]);
    let learning = contractnet::bidder::Learning::default();
    let context = BidContext {
        rules: &rules,
        rates: &rates,
        history: &[],
        queue_seconds: 2.0,
        learning: &learning,
        market: None,
        name: "Rust_07",
    };
    let mut task = Task {
        task_id: 1,
        task_type: "monte_carlo_pi".into(),
        params: json!({"seed":1,"samples":1000}),
        budget: 20.0,
        deadline_s: 4.0,
        bid_window_ms: 100,
        attempt: 1,
    };
    let bid = MyContractor.on_cfp(&task, &context).unwrap();
    assert!(bid.price > bid.est_seconds * rules.cost_rate);
    assert!(bid.est_seconds > 3.0);
    task.deadline_s = 2.0;
    assert!(MyContractor.on_cfp(&task, &context).is_none());
    task.deadline_s = 4.0;
    task.budget = 4.0;
    assert!(MyContractor.on_cfp(&task, &context).is_none());
    task.budget = 10.0;
    task.task_type = "unknown".into();
    assert!(MyContractor.on_cfp(&task, &context).is_none());
}
