//! Use archived bids only when task metadata and the recorded result match.
use super::{MeasuredTask, Measurements};
use anyhow::{Result, ensure};
use contractnet::{
    Rules,
    market::{Auction, MarketBid, Snapshot},
};
use serde::Deserialize;
use std::{fs, path::Path};

#[derive(Deserialize)]
struct History {
    rules: Rules,
    auctions: Vec<RecordedAuction>,
}
#[derive(Deserialize)]
struct RecordedAuction {
    task_id: u64,
    task_type: String,
    budget: f64,
    deadline_s: f64,
    proposed_at: u64,
    accepted_at: u64,
    agent: String,
    price: f64,
    est_seconds: f64,
    result: String,
}

pub(super) fn cases(data: &Measurements, path: &Path) -> Result<Vec<MeasuredTask>> {
    let mut history: History = serde_json::from_slice(&fs::read(path)?)?;
    history.auctions.sort_by_key(|a| a.proposed_at);
    let mut output = Vec::new();
    for record in history.auctions {
        let matched = data.tasks.iter().find(|m| {
            m.task.task_id == record.task_id
                && m.task.task_type == record.task_type
                && m.task.budget == record.budget
                && m.task.deadline_s == record.deadline_s
                && m.result == record.result
        });
        let Some(m) = matched else {
            continue;
        };
        let mut m = m.clone();
        m.market = Snapshot {
            now: record.proposed_at,
            config: history.rules.clone(),
            active: vec![Auction {
                task: m.task.clone(),
                state: "bidding".into(),
                bids_close_at: Some(record.accepted_at),
            }],
            live_bids: vec![MarketBid {
                task_id: record.task_id,
                agent: record.agent,
                price: record.price,
                est_seconds: record.est_seconds,
                outcome: None,
            }],
        };
        if m.market
            .competing_score(&m.task, &m.market.config, "TheGoodGuys")
            .is_some()
        {
            output.push(m);
        }
    }
    ensure!(
        !output.is_empty(),
        "no old auctions matched metadata and recorded task results"
    );
    Ok(output)
}
