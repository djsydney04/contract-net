//! Read-only public auction observations. This connection never registers or bids.
use crate::{Rules, Task};
use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::{
    sync::watch,
    time::{sleep, timeout},
};
use tokio_tungstenite::{
    connect_async_with_config,
    tungstenite::{Message, http::Uri},
};

#[derive(Clone, Debug, Serialize)]
pub struct Auction {
    pub task: Task,
    pub state: String,
    pub bids_close_at: Option<u64>,
}

impl<'de> Deserialize<'de> for Auction {
    fn deserialize<D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> std::result::Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        #[derive(Deserialize)]
        struct Meta {
            state: String,
            bids_close_at: Option<u64>,
        }
        let meta: Meta = serde_json::from_value(value.clone()).map_err(serde::de::Error::custom)?;
        // Saved captures nest task data; the live feed sends flat CFP fields.
        let task = serde_json::from_value(value.get("task").cloned().unwrap_or(value))
            .map_err(serde::de::Error::custom)?;
        Ok(Self {
            task,
            state: meta.state,
            bids_close_at: meta.bids_close_at,
        })
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct MarketBid {
    pub task_id: u64,
    pub agent: String,
    pub price: f64,
    pub est_seconds: f64,
    pub outcome: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Snapshot {
    pub now: u64,
    #[serde(default)]
    pub config: Rules,
    #[serde(default)]
    pub active: Vec<Auction>,
    #[serde(default)]
    pub live_bids: Vec<MarketBid>,
}

impl Snapshot {
    pub fn parse(raw: &str) -> Result<Self> {
        // Preserve JSON decimal numbers before decoding the flattened task.
        let value: serde_json::Value = serde_json::from_str(raw)?;
        serde_json::from_value(value).context("invalid public market snapshot")
    }

    pub fn competing_score(&self, task: &Task, rules: &Rules, name: &str) -> Option<f64> {
        if self.config.award_policy != rules.award_policy
            || self.config.time_weight != rules.time_weight
        {
            return None;
        }
        let auction = self.active.iter().find(|a| {
            a.task.task_id == task.task_id
                && a.task.task_type == task.task_type
                && a.task.params == task.params
                && a.task.attempt == task.attempt
                && a.task.budget == task.budget
                && a.task.deadline_s == task.deadline_s
                && a.state == "bidding"
                && a.bids_close_at
                    .is_some_and(|close| close > self.now.saturating_add(200))
        })?;
        self.live_bids
            .iter()
            .filter(|b| {
                b.task_id == auction.task.task_id
                    && b.agent != name
                    && b.outcome.is_none()
                    && b.price.is_finite()
                    && b.price >= 0.0
                    && b.price <= task.budget
                    && b.est_seconds.is_finite()
                    && b.est_seconds >= 0.0
                    && b.est_seconds <= task.deadline_s
            })
            .filter_map(|b| score(rules, b.price, b.est_seconds))
            .min_by(f64::total_cmp)
    }
}

pub fn score(rules: &Rules, price: f64, seconds: f64) -> Option<f64> {
    let value = match rules.award_policy.as_str() {
        "best_value" if rules.time_weight.is_finite() && rules.time_weight >= 0.0 => {
            price + rules.time_weight * seconds
        }
        "lowest_price" | "cheapest" => price,
        _ => return None,
    };
    value.is_finite().then_some(value)
}

pub fn spectator_url(agent_url: &str) -> Option<String> {
    let uri: Uri = agent_url.parse().ok()?;
    let authority = uri.authority()?;
    (uri.path() == "/agent").then(|| {
        format!(
            "{}://{}/spectate{}",
            uri.scheme_str().unwrap_or("wss"),
            authority,
            uri.query().map(|q| format!("?{q}")).unwrap_or_default()
        )
    })
}

pub fn subscribe(url: String) -> watch::Receiver<Option<Snapshot>> {
    let (sender, receiver) = watch::channel(None);
    tokio::spawn(async move {
        while !sender.is_closed() {
            if let Ok(Ok((mut socket, _))) = timeout(
                Duration::from_secs(10),
                connect_async_with_config(&url, None, true),
            )
            .await
            {
                loop {
                    tokio::select! {
                        _ = sender.closed() => return,
                        frame = timeout(Duration::from_secs(45), socket.next()) => match frame {
                            Ok(Some(Ok(Message::Text(raw)))) => {
                                if let Ok(snapshot) = Snapshot::parse(&raw) {
                                    sender.send_replace(Some(snapshot));
                                }
                            }
                            Ok(Some(Ok(Message::Ping(_)))) => { if socket.flush().await.is_err() { break; } }
                            Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Err(_) | Ok(Some(Err(_))) => break,
                            _ => {},
                        }
                    }
                }
            }
            sender.send_replace(None);
            tokio::select! {
                _ = sender.closed() => return,
                _ = sleep(Duration::from_secs(3)) => {},
            }
        }
    });
    receiver
}
