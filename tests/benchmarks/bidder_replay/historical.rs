//! Use archived bids only when task metadata and the recorded result match.
use super::{MeasuredTask, Measurements};
use anyhow::{Result, ensure};
use contractnet::{
    Rules,
    market::{Auction, MarketBid, Snapshot},
};
use serde::{Deserialize, Serialize};
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
    local_seconds: f64,
    manager_seconds: f64,
}

#[derive(Debug, Serialize)]
pub(super) struct DeliveryRecord {
    pub task_id: u64,
    pub task_type: String,
    pub proposed_at: u64,
    pub agent: String,
    pub local_seconds: f64,
    pub manager_seconds: f64,
    pub residual_ms: f64,
}

pub(super) struct Cases {
    pub tasks: Vec<MeasuredTask>,
    pub delivery: Vec<DeliveryRecord>,
}

pub(super) fn cases(data: &Measurements, path: &Path) -> Result<Cases> {
    let mut history: History = serde_json::from_slice(&fs::read(path)?)?;
    history.auctions.sort_by_key(|a| a.proposed_at);
    let mut output = Vec::new();
    let mut delivery = Vec::new();
    for record in history.auctions {
        ensure!(
            record.local_seconds.is_finite()
                && record.manager_seconds.is_finite()
                && record.local_seconds >= 0.0
                && record.manager_seconds >= record.local_seconds,
            "invalid historical timing for task #{}",
            record.task_id
        );
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
                agent: record.agent.clone(),
                price: record.price,
                est_seconds: record.est_seconds,
                outcome: None,
            }],
        };
        if m.market
            .competing_score(&m.task, &m.market.config, "TheGoodGuys")
            .is_some()
        {
            delivery.push(DeliveryRecord {
                task_id: record.task_id,
                task_type: record.task_type,
                proposed_at: record.proposed_at,
                agent: record.agent,
                local_seconds: record.local_seconds,
                manager_seconds: record.manager_seconds,
                residual_ms: (record.manager_seconds - record.local_seconds) * 1000.0,
            });
            output.push(m);
        }
    }
    ensure!(
        !output.is_empty(),
        "no old auctions matched metadata and recorded task results"
    );
    Ok(Cases {
        tasks: output,
        delivery,
    })
}
